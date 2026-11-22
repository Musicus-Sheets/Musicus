//! Versions, release lines and the downgrade window (R-OPS-014,
//! d-update-model). Pure functions; the runner (`runner.rs`) feeds them
//! with the marker read from the database.

use std::fmt;

/// A release version `major.minor.patch`. A pre-release or build suffix
/// (`1.2.0-rc.1`, `1.2.0+abc`) is ignored: it orders like the release.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

/// A release line `major.minor` (a minor release and all its patches).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Line {
    pub major: u32,
    pub minor: u32,
}

impl Version {
    pub fn parse(text: &str) -> Option<Self> {
        let core = text.trim().split(['-', '+']).next()?;
        let mut parts = core.split('.');
        let num = |p: Option<&str>| p.filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))?.parse().ok();
        let v = Version { major: num(parts.next())?, minor: num(parts.next())?, patch: num(parts.next())? };
        parts.next().is_none().then_some(v)
    }

    pub fn line(self) -> Line {
        Line { major: self.major, minor: self.minor }
    }
}

impl Line {
    pub fn parse(text: &str) -> Option<Self> {
        let (major, minor) = text.trim().split_once('.')?;
        let num = |s: &str| s.bytes().all(|b| b.is_ascii_digit()).then(|| s.parse().ok()).flatten();
        Some(Line { major: num(major)?, minor: num(minor)? })
    }

    /// The first release of the line (`X.Y.0`).
    pub fn first(self) -> Version {
        Version { major: self.major, minor: self.minor, patch: 0 }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl fmt::Display for Line {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// Parses `releases.txt`: one released line `X.Y` per line, in release order;
/// `#` starts a comment. Errors name the offending line.
pub fn parse_releases(text: &str) -> Result<Vec<Line>, String> {
    let mut lines: Vec<Line> = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let entry = raw.split('#').next().unwrap_or("").trim();
        if entry.is_empty() {
            continue;
        }
        let line = Line::parse(entry).ok_or_else(|| format!("releases.txt:{}: `{entry}` is not `X.Y`", n + 1))?;
        if lines.last().is_some_and(|last| *last >= line) {
            return Err(format!("releases.txt:{}: {line} is not newer than the line before", n + 1));
        }
        lines.push(line);
    }
    Ok(lines)
}

/// One change of the program version on a database (`musicus_upgrades`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Upgrade {
    pub from: Version,
    pub to: Version,
    /// Unix seconds of the start that performed it.
    pub at: i64,
}

/// The releases that may still be started on a database (R-OPS-014), seen by
/// the newest release that ran on it (`current`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    pub current: Line,
    /// Rule (a): the three release lines released before `current`.
    pub previous: Vec<Line>,
    /// Rule (b): releases the installation was upgraded from less than three
    /// months ago.
    pub sources: Vec<Version>,
}

/// Number of earlier release lines that stay downgrade targets (rule a).
pub const PREVIOUS_LINES: usize = 3;
/// Months an upgrade source stays a downgrade target (rule b).
pub const SOURCE_MONTHS: u32 = 3;

impl Window {
    /// `releases` are the released lines in release order (`releases.txt`);
    /// `current` need not be listed (an unreleased development line counts as
    /// the line after the last listed one). Counting is by release order, not
    /// by number: after 1.8, 1.9, 1.10 come the targets of 2.0.
    pub fn new(releases: &[Line], current: Line, upgrades: &[Upgrade], now: i64) -> Self {
        let earlier: Vec<Line> = releases.iter().copied().filter(|l| *l < current).collect();
        let previous = earlier[earlier.len().saturating_sub(PREVIOUS_LINES)..].to_vec();
        let mut sources: Vec<Version> = upgrades
            .iter()
            .filter(|u| u.from < u.to && now < add_months(u.at, SOURCE_MONTHS))
            .map(|u| u.from)
            .collect();
        sources.sort();
        sources.dedup();
        Window { current, previous, sources }
    }

    /// Whether `version` may still be started (rules a, b, c).
    pub fn contains(&self, version: Version) -> bool {
        let line = version.line();
        line == self.current || self.previous.contains(&line) || self.sources.contains(&version)
    }

    /// The oldest line the window keeps: a removal brought by a release of
    /// this line or older is due.
    pub fn floor(&self) -> Line {
        let mut floor = self.previous.first().copied().unwrap_or(self.current);
        for s in &self.sources {
            floor = floor.min(s.line());
        }
        floor
    }
}

/// `at` plus `months` calendar months (UTC), the day clamped to the length of
/// the target month: 1 March + 3 = 1 June, 30 November + 3 = 28/29 February.
pub fn add_months(at: i64, months: u32) -> i64 {
    let days = at.div_euclid(86_400);
    let secs = at.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let total = m as i64 - 1 + months as i64;
    let (y, m) = (y + total.div_euclid(12), (total.rem_euclid(12) + 1) as u32);
    let d = d.min(days_in_month(y, m));
    days_from_civil(y, m, d) * 86_400 + secs
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        4 | 6 | 9 | 11 => 30,
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        _ => 31,
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date (H. Hinnant's algorithm).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }
    fn l(s: &str) -> Line {
        Line::parse(s).unwrap()
    }
    fn lines(list: &[&str]) -> Vec<Line> {
        list.iter().map(|s| l(s)).collect()
    }
    fn date(y: i64, m: u32, d: u32) -> i64 {
        days_from_civil(y, m, d) * 86_400
    }

    #[test]
    fn versions_parse_and_order() {
        assert_eq!(v("1.10.2"), Version { major: 1, minor: 10, patch: 2 });
        assert_eq!(v("1.0.0-rc.1"), v("1.0.0"));
        assert!(v("1.9.3") < v("1.10.0") && v("1.10.0") < v("2.0.0"));
        assert_eq!(Version::parse("1.2"), None);
        assert_eq!(Version::parse("1.2.3.4"), None);
        assert_eq!(Version::parse("a.b.c"), None);
        assert_eq!(v("1.5.4").line().to_string(), "1.5");
    }

    #[test]
    fn release_list_is_ordered() {
        assert_eq!(
            parse_releases("# released lines\n1.0\n1.1 # first feature release\n\n").unwrap(),
            lines(&["1.0", "1.1"])
        );
        assert!(parse_releases("1.1\n1.0\n").unwrap_err().contains("releases.txt:2"));
        assert!(parse_releases("1.0.0\n").is_err());
    }

    #[test]
    fn calendar_months() {
        assert_eq!(add_months(date(2027, 3, 1), 3), date(2027, 6, 1));
        assert_eq!(add_months(date(2027, 11, 30) + 3600, 3), date(2028, 2, 29) + 3600);
        assert_eq!(add_months(date(2026, 11, 30), 3), date(2027, 2, 28));
        assert_eq!(add_months(date(2027, 10, 15), 3), date(2028, 1, 15));
        assert_eq!(civil_from_days(days_from_civil(2000, 2, 29)), (2000, 2, 29));
        assert_eq!(days_from_civil(1970, 1, 1), 0);
    }

    /// Chapter 14: 1.0.3 upgraded directly to 1.5.0 on 1 March.
    #[test]
    fn upgrade_source_stays_a_target_for_three_months() {
        let releases = lines(&["1.0", "1.1", "1.2", "1.3", "1.4", "1.5"]);
        let upgrades = [Upgrade { from: v("1.0.3"), to: v("1.5.0"), at: date(2027, 3, 1) + 10 * 3600 }];
        let before = Window::new(&releases, l("1.5"), &upgrades, date(2027, 5, 31) + 12 * 3600);
        for ok in ["1.0.3", "1.2.0", "1.2.7", "1.3.1", "1.4.0", "1.5.0", "1.5.4"] {
            assert!(before.contains(v(ok)), "{ok} until 1 June");
        }
        for no in ["1.0.0", "1.0.2", "1.1.0"] {
            assert!(!before.contains(v(no)), "{no} was never the source");
        }
        assert_eq!(before.floor(), l("1.0"));
        let after = Window::new(&releases, l("1.5"), &upgrades, date(2027, 6, 1) + 10 * 3600);
        assert!(!after.contains(v("1.0.3")), "from 1 June on only 1.2 to 1.4");
        assert!(after.contains(v("1.2.0")) && after.contains(v("1.4.9")));
        assert_eq!(after.floor(), l("1.2"));
    }

    /// Release order, not numbering: 1.8 → 1.9 → 1.10 → 2.0.
    #[test]
    fn previous_lines_count_in_release_order() {
        let releases = lines(&["1.7", "1.8", "1.9", "1.10", "2.0"]);
        let w = Window::new(&releases, l("2.0"), &[], 0);
        assert_eq!(w.previous, lines(&["1.8", "1.9", "1.10"]));
        assert!(!w.contains(v("1.7.3")));
        assert_eq!(w.floor(), l("1.8"));
        // Gaps in the numbering do not count either.
        let w = Window::new(&lines(&["1.0", "1.2", "1.5", "3.0"]), l("3.0"), &[], 0);
        assert_eq!(w.previous, lines(&["1.0", "1.2", "1.5"]));
    }

    /// 1.0 → 1.5 → 1.6: each source stays a target for three months after its own upgrade.
    #[test]
    fn chained_upgrades() {
        let releases = lines(&["1.0", "1.1", "1.2", "1.3", "1.4", "1.5", "1.6"]);
        let upgrades = [
            Upgrade { from: v("1.0.1"), to: v("1.5.0"), at: date(2027, 1, 10) },
            Upgrade { from: v("1.5.0"), to: v("1.6.0"), at: date(2027, 3, 10) },
        ];
        let w = Window::new(&releases, l("1.6"), &upgrades, date(2027, 4, 1));
        assert!(w.contains(v("1.0.1")) && w.contains(v("1.5.0")) && w.contains(v("1.3.0")));
        assert!(!w.contains(v("1.2.0")));
        assert_eq!(w.floor(), l("1.0"));
        let w = Window::new(&releases, l("1.6"), &upgrades, date(2027, 4, 10));
        assert!(!w.contains(v("1.0.1")), "three months after 10 January");
        assert_eq!(w.floor(), l("1.3"));
        // Downgrades are no sources.
        let back = [Upgrade { from: v("1.6.0"), to: v("1.0.1"), at: date(2027, 4, 9) }];
        assert_eq!(Window::new(&releases, l("1.6"), &back, date(2027, 4, 10)).floor(), l("1.3"));
    }

    /// Rule (c): patches of the same line, however many lie between.
    #[test]
    fn patches_of_the_same_line() {
        let releases = lines(&["1.4", "1.5"]);
        let w = Window::new(&releases, l("1.5"), &[], 0);
        assert!(w.contains(v("1.5.0")) && w.contains(v("1.5.7")) && w.contains(v("1.5.99")));
        assert!(w.contains(v("1.4.0")));
    }

    /// Fewer than three earlier lines; an unreleased development line.
    #[test]
    fn early_and_development_lines() {
        let w = Window::new(&[], l("0.1"), &[], 0);
        assert!(w.previous.is_empty());
        assert_eq!(w.floor(), l("0.1"));
        let w = Window::new(&lines(&["1.0", "1.1"]), l("1.2"), &[], 0);
        assert_eq!(w.previous, lines(&["1.0", "1.1"]), "1.2 not listed yet: the next line");
        assert_eq!(w.floor(), l("1.0"));
    }
}
