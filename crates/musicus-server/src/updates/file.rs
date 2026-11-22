//! The format of a database update file (`migrations/<version>.sql`,
//! `migrations/preview/NNNN-<topic>.sql`), a-update-runner:
//!
//! ```sql
//! -- What the update does (comments, free text).
//! -- @previews: 0001-topic 0002-other      (release updates merged from previews only)
//! -- @additive
//! CREATE TABLE ...;                        (runs at the first start of the release)
//! -- @rebuild
//! DELETE FROM new; INSERT INTO new ...;    (runs after an older release used the data)
//! -- @removal
//! ALTER TABLE t DROP COLUMN old;           (runs when no release in the window needs it)
//! ```
//!
//! A marker is a line that is exactly `-- @additive`, `-- @rebuild` or
//! `-- @removal` (surrounding blanks ignored). `-- @additive` is required; the
//! others are optional; the order is fixed. Any other `-- @` line is an error,
//! so a typo cannot silently put a removal into the additive part. The text
//! before the first marker may hold only comments, blank lines and the
//! `-- @previews:` line. The same parser exists in
//! `scripts/release-migrations.py`; keep both in step.

/// The parts of one update file, as slices of the embedded text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sections {
    pub previews: Vec<&'static str>,
    pub additive: &'static str,
    pub rebuild: &'static str,
    pub removal: &'static str,
}

const MARKERS: [&str; 3] = ["-- @additive", "-- @rebuild", "-- @removal"];
const PREVIEWS: &str = "-- @previews:";

pub fn parse(file: &str, text: &'static str) -> Result<Sections, String> {
    let mut out = Sections::default();
    // (section index, byte offset where its body starts)
    let mut current: Option<(usize, usize)> = None;
    let mut offset = 0;
    for (n, raw) in text.split_inclusive('\n').enumerate() {
        let line = raw.trim();
        let at = |what: &str| format!("{file}:{}: {what}", n + 1);
        if let Some(i) = MARKERS.iter().position(|m| line == *m) {
            if current.is_some_and(|(c, _)| c >= i) {
                return Err(at("sections must appear once each, in the order additive, rebuild, removal"));
            }
            if current.is_none() && i != 0 {
                return Err(at("the first section must be `-- @additive`"));
            }
            if let Some((c, start)) = current {
                set(&mut out, c, &text[start..offset]);
            }
            current = Some((i, offset + raw.len()));
        } else if let Some(list) = line.strip_prefix(PREVIEWS) {
            if current.is_some() || !out.previews.is_empty() {
                return Err(at("`-- @previews:` belongs once into the header, before `-- @additive`"));
            }
            out.previews = list.split([' ', ',']).filter(|s| !s.is_empty()).collect();
            if out.previews.is_empty() {
                return Err(at("`-- @previews:` lists no preview"));
            }
        } else if line.starts_with("-- @") {
            return Err(at(&format!("unknown marker `{line}`")));
        } else if current.is_none() && !line.is_empty() && !line.starts_with("--") {
            return Err(at("SQL before `-- @additive`"));
        }
        offset += raw.len();
    }
    match current {
        Some((c, start)) => set(&mut out, c, &text[start..]),
        None => return Err(format!("{file}: no `-- @additive` section")),
    }
    Ok(out)
}

fn set(out: &mut Sections, section: usize, body: &'static str) {
    match section {
        0 => out.additive = body,
        1 => out.rebuild = body,
        _ => out.removal = body,
    }
}

/// True if the SQL holds nothing but comments and blanks.
pub fn is_empty(sql: &str) -> bool {
    sql.lines().map(str::trim).all(|l| l.is_empty() || l.starts_with("--"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_and_header() {
        let s = parse(
            "1.1.0.sql",
            "-- Renames a to b.\n-- @previews: 0001-b, 0002-c\n\n-- @additive\nALTER TABLE t ADD b;\n-- @rebuild\nUPDATE t SET b = a;\n  -- @removal  \nALTER TABLE t DROP a;\n",
        )
        .unwrap();
        assert_eq!(s.previews, ["0001-b", "0002-c"]);
        assert_eq!(s.additive, "ALTER TABLE t ADD b;\n");
        assert_eq!(s.rebuild, "UPDATE t SET b = a;\n");
        assert_eq!(s.removal, "ALTER TABLE t DROP a;\n");
        let s = parse("x.sql", "-- @additive\nCREATE TABLE t (a);").unwrap();
        assert_eq!((s.additive, s.rebuild, s.removal), ("CREATE TABLE t (a);", "", ""));
        assert!(s.previews.is_empty());
    }

    #[test]
    fn mistakes_are_refused() {
        let err = |text: &'static str| parse("p.sql", text).unwrap_err();
        assert!(err("-- only a comment\n").contains("no `-- @additive`"));
        assert!(err("CREATE TABLE t (a);\n-- @additive\n").contains("p.sql:1: SQL before"));
        assert!(err("-- @removal\nDROP TABLE t;\n").contains("first section"));
        assert!(err("-- @additive\n-- @removal\n-- @rebuild\n").contains("p.sql:3"));
        assert!(err("-- @additive\n-- @additive\n").contains("once each"));
        assert!(err("-- @additive\n-- @remove\nDROP TABLE t;\n").contains("unknown marker `-- @remove`"));
        assert!(err("-- @additive\n-- @previews: 0001-a\n").contains("header"));
        assert!(err("-- @previews:\n-- @additive\n").contains("lists no preview"));
    }

    #[test]
    fn empty_parts() {
        assert!(is_empty("\n-- nothing to remove\n  \n"));
        assert!(!is_empty("-- drop\nDROP TABLE t;"));
    }
}
