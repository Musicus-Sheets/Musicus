//! Music-system detection on a greyscale page raster (pure computation, no I/O).
//!
//! The exact algorithm, with every constant below, is documented in
//! `docs/technical/chapters/12-system-view.typ` and `docs/agents/layout.md`.
//! Steps:
//! 1. Otsu binarisation.
//! 2. Vertical run-length histograms → staff-line thickness `t` and line distance `d`.
//! 3. Thin pixels (vertical runs ≤ ~2t) → skew estimate; vertical shear to deskew.
//! 4. Line evidence = ink in horizontal runs ≥ 1.5 d (staff lines stay long runs through
//!    note heads and stems, text does not).
//! 5. Per vertical strip (8 d wide): line candidates → groups of 5 (or 6 = tablature)
//!    equally spaced lines.
//! 6. Link strip groups left → right into staves; merge broken tracks; drop short ones.
//! 7. Staves connected at their left end (barline / brace / bracket) or by a barline form a
//!    system.
//! 8. Cuts between systems at the longest ink-free row run (else minimum ink); header above
//!    the first system, footer below the last (ink right below the last staff stays with it).
//! 9. Confidence from consistency checks.

use super::Gray;

// --- Constants (named in the technical documentation, chapter "System view") ---
// Lengths given in "spaces" are multiples of the page's line distance `d`
// (distance between two staff lines, centre to centre).

/// Otsu threshold is clamped to this range (grey values, 0 = black).
pub const OTSU_MIN: u8 = 60;
pub const OTSU_MAX: u8 = 210;
/// A vertical ink run up to `THIN_RUN_FACTOR * t + 1` (at least `t + THIN_RUN_EXTRA`) is
/// "thin", i.e. may belong to a staff line (`t` = line thickness). Used for the skew estimate.
pub const THIN_RUN_FACTOR: usize = 2;
pub const THIN_RUN_EXTRA: usize = 3;
/// Skew search range and steps, degrees.
pub const SKEW_MAX_DEG: f32 = 3.0;
pub const SKEW_COARSE_STEP: f32 = 0.1;
pub const SKEW_FINE_STEP: f32 = 0.01;
/// Below this skew the page is not sheared.
pub const SKEW_IGNORE_DEG: f32 = 0.02;
/// Line evidence: horizontal ink runs of at least this many spaces (min. 8 px); gaps of
/// `LONG_RUN_BRIDGE_PX` are closed.
pub const LONG_RUN_SPACES: f32 = 1.5;
pub const LONG_RUN_MIN_PX: usize = 8;
pub const LONG_RUN_BRIDGE_PX: usize = 1;
/// Width of the vertical strips in which staves are searched, in spaces (32 px .. w/4).
pub const STRIP_SPACES: f32 = 8.0;
/// A strip row is a line candidate if this fraction of it is line evidence.
pub const MIN_LINE_FILL: f32 = 0.5;
/// Line spacing accepted for a group: `max(GROUP_MIN_SPACING * d, 3 px) ..= GROUP_MAX_SPACING * d`.
pub const GROUP_MIN_SPACING: f32 = 0.4;
pub const GROUP_MAX_SPACING: f32 = 2.5;
/// Position tolerance of a line in a group: `max(LINE_TOL * g, 1.5 px)` (`g` = group spacing).
pub const LINE_TOL: f32 = 0.2;
/// The weakest line of a group must have at least this fraction of the median line's strength.
pub const MIN_LINE_STRENGTH: f32 = 0.45;
/// Candidates within `MID_LINE_TOL * g` of the middle of two group lines (in two or more
/// gaps) mean the group takes every other line of two close staves: rejected.
pub const MID_LINE_TOL: f32 = 0.2;
/// A sixth line of at least this relative strength makes a tablature staff ...
pub const TAB_LINE_STRENGTH: f32 = 0.7;
/// ... if its spacing is at least this multiple of the page's normal staves.
pub const TAB_MIN_SPACING: f32 = 1.25;
/// Linking strip groups into staves: at most this many strips apart, top line within
/// `TRACK_Y_TOL * g`, spacing within `TRACK_SPACING_TOL * g`.
pub const TRACK_MAX_GAP_STRIPS: usize = 4;
pub const TRACK_Y_TOL: f32 = 0.6;
pub const TRACK_SPACING_TOL: f32 = 0.2;
/// Staves shorter than this many spaces are dropped.
pub const MIN_STAFF_SPACES: f32 = 12.0;
/// Horizontal extent: a column belongs to the staff while all lines but one have ink; gaps
/// up to this many spaces are bridged.
pub const EXTENT_GAP_SPACES: f32 = 0.6;
/// Two staves are one system if the rows between them are inked to this fraction in the
/// window `[left - CONNECT_LEFT_SPACES, left + CONNECT_RIGHT_SPACES]` at their left edge ...
pub const MIN_CONNECT: f32 = 0.9;
pub const CONNECT_LEFT_SPACES: f32 = 3.0;
pub const CONNECT_RIGHT_SPACES: f32 = 1.0;
/// ... or a column (± half a line thickness) anywhere is inked to this fraction (a barline).
pub const BARLINE_COVER: f32 = 0.97;
/// A row is ink-free if it has at most this fraction of the content width inked (min 1 px).
pub const NOISE_FRACTION: f32 = 0.004;
/// A cut through ink is harmless up to this fraction of the content width.
pub const CUT_INK_OK: f32 = 0.02;
/// Above the first / below the last system: ink-free runs score `length / (1 + distance /
/// (MARGIN_DECAY_SPACES * d))`; runs touching the page edge only win if there is no other.
pub const MARGIN_DECAY_SPACES: f32 = 8.0;
/// Without any ink-free run, the cut is searched within `max(MARGIN_GAP_FACTOR *
/// typical gap, MARGIN_MIN_SPACES * d)` of the staff.
pub const MARGIN_GAP_FACTOR: f32 = 0.6;
pub const MARGIN_MIN_SPACES: f32 = 5.0;
/// The row profile for cuts covers the staves plus this many spaces left and right.
pub const CONTENT_MARGIN_SPACES: f32 = 2.0;
/// A column has ink in a region if more than this fraction of its rows (min 1) is inked.
pub const INK_COLUMN_FILL: f32 = 0.002;
/// A margin cut with no header/footer ink lies this many spaces from the ink.
pub const EDGE_CUT_SPACES: f32 = 2.0;
/// Typical gap between systems if a page has only one: this many spaces.
pub const DEFAULT_GAP_SPACES: f32 = 8.0;
/// Rows at the top/bottom quarter that are more than half inked are scanner border.
pub const BORDER_ROW_FILL: f32 = 0.5;
/// Ink below the last system that starts within this many spaces of its bottom line
/// (8va/8vb lines, low notes, pedal marks) belongs to the system, up to the next ink-free
/// run of one space.
pub const FOOTER_ATTACH_SPACES: f32 = 4.0;
/// Horizontal ink extent of a system: from `SYSTEM_LEFT_SPACES` left of its staves
/// (instrument names, braces) to `SYSTEM_RIGHT_SPACES` right of them (text after the last barline).
pub const SYSTEM_LEFT_SPACES: f32 = 12.0;
pub const SYSTEM_RIGHT_SPACES: f32 = 6.0;
/// Confidence penalties (factors).
pub const PENALTY_UNCLEAN_CUT: f32 = 0.85;
pub const PENALTY_MISSED_STAFF: f32 = 0.5;
pub const PENALTY_FAR_INK: f32 = 0.9;
pub const PENALTY_STAFF_COUNT: f32 = 0.8;
pub const PENALTY_WIDTHS: f32 = 0.7;
pub const PENALTY_SPACING: f32 = 0.9;
pub const PENALTY_SKEW: f32 = 0.9;
/// Confidence of a page with ink but no staff.
pub const NO_STAFF_CONFIDENCE: f32 = 0.2;
/// A page with less ink than this fraction is blank.
pub const BLANK_INK: f32 = 0.003;
/// Ink "near" a staff for the missed-staff check: within this many spaces.
pub const NEAR_STAFF_SPACES: f32 = 7.0;
/// Missed staff: three or more line-like rows (long runs over this fraction of the content
/// width) far from the staves of a system.
pub const MISSED_LINE_FILL: f32 = 0.25;
/// Ink far from staves: rows with more than this fraction inked, for over two spaces.
pub const FAR_INK_FILL: f32 = 0.05;
/// Staves of one system narrower than this fraction of the widest one lower confidence.
pub const MIN_WIDTH_RATIO: f32 = 0.85;
/// Line distances further than this from the page median lower confidence.
pub const SPACING_DEVIATION: f32 = 0.3;
/// Skew above this lowers confidence (degrees).
pub const SKEW_WARN_DEG: f32 = 1.5;

/// A staff in deskewed pixel coordinates.
#[derive(Clone, Debug)]
pub struct Staff {
    /// 5 = normal (incl. percussion), 6 = tablature.
    pub lines: usize,
    pub line_y: Vec<f32>,
    /// Line distance in px.
    pub space: f32,
    pub x0: f32,
    pub x1: f32,
    /// Number of strips in which the staff was found.
    pub strips: usize,
    samples: Vec<(f32, f32)>,
}

impl Staff {
    pub fn top(&self) -> f32 {
        self.line_y[0]
    }
    pub fn bottom(&self) -> f32 {
        *self.line_y.last().expect("a staff has lines")
    }
    /// Vertical offset of the staff at column x relative to its median position (curvature).
    fn offset_at(&self, x: f32) -> f32 {
        let mut best = (f32::MAX, 0.0);
        for &(sx, top) in &self.samples {
            let dx = (sx - x).abs();
            if dx < best.0 {
                best = (dx, top - self.top());
            }
        }
        best.1
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Header,
    System,
    Footer,
}

/// A region of the page. Coordinates are page-relative (0..1) in the original (not deskewed)
/// page; with skew they are the bounding box of the slanted region.
#[derive(Clone, Debug)]
pub struct Region {
    pub kind: Kind,
    /// Cut lines above/below.
    pub y0: f32,
    pub y1: f32,
    /// Ink extent inside the cut lines.
    pub ink_y0: f32,
    pub ink_y1: f32,
    pub x0: f32,
    pub x1: f32,
    pub staves: usize,
    /// Line distance relative to the page height (0 for header/footer).
    pub staff_space: f32,
    /// Cut above/below went through (almost) ink-free rows.
    pub clean_top: bool,
    pub clean_bottom: bool,
}

#[derive(Clone, Debug)]
pub struct PageResult {
    pub width_px: usize,
    pub height_px: usize,
    pub skew_deg: f32,
    pub line_thickness_px: f32,
    pub line_distance_px: f32,
    pub staves: Vec<Staff>,
    pub regions: Vec<Region>,
    pub systems: usize,
    pub confidence: f32,
    /// Reasons for a lowered confidence (for logs and tests).
    pub issues: Vec<String>,
}

/// Binary image, 1 = ink.
struct Bin {
    w: usize,
    h: usize,
    px: Vec<u8>,
}

impl Bin {
    #[inline]
    fn at(&self, x: usize, y: usize) -> bool {
        self.px[y * self.w + x] != 0
    }
}

fn otsu(g: &Gray) -> u8 {
    let mut hist = [0u64; 256];
    for &v in &g.px {
        hist[v as usize] += 1;
    }
    let total = g.px.len() as f64;
    let sum: f64 = hist.iter().enumerate().map(|(i, &c)| i as f64 * c as f64).sum();
    let (mut wb, mut sb, mut best, mut thr) = (0.0, 0.0, -1.0, 128u8);
    for (t, &count) in hist.iter().enumerate() {
        wb += count as f64;
        if wb == 0.0 {
            continue;
        }
        let wf = total - wb;
        if wf == 0.0 {
            break;
        }
        sb += t as f64 * count as f64;
        let mb = sb / wb;
        let mf = (sum - sb) / wf;
        let between = wb * wf * (mb - mf) * (mb - mf);
        if between > best {
            best = between;
            thr = t as u8;
        }
    }
    thr.clamp(OTSU_MIN, OTSU_MAX)
}

/// Returns (line thickness, line distance) from vertical run-length histograms.
fn run_lengths(b: &Bin) -> (usize, usize) {
    let mut hb = [0u64; 64];
    let mut hp = [0u64; 160];
    for x in (0..b.w).step_by(2) {
        let (mut y, mut prev_black) = (0, None::<usize>);
        while y < b.h {
            let v = b.at(x, y);
            let s = y;
            while y < b.h && b.at(x, y) == v {
                y += 1;
            }
            let len = y - s;
            if v {
                hb[len.min(63)] += 1;
                prev_black = Some(len);
            } else if let Some(bl) = prev_black.take()
                && y < b.h
            {
                hp[(bl + len).min(159)] += 1;
            }
        }
    }
    let t = (1..20).max_by_key(|&i| hb[i]).unwrap_or(2);
    let d = (4..150).max_by_key(|&i| hp[i - 1] + 2 * hp[i] + hp[i + 1]).unwrap_or(12);
    (t, d)
}

/// Ink pixels that belong to vertical runs of at most `tmax` pixels.
fn line_pixels(b: &Bin, tmax: usize) -> Bin {
    let mut out = vec![0u8; b.w * b.h];
    for x in 0..b.w {
        let mut y = 0;
        while y < b.h {
            if !b.at(x, y) {
                y += 1;
                continue;
            }
            let s = y;
            while y < b.h && b.at(x, y) {
                y += 1;
            }
            if y - s <= tmax {
                for yy in s..y {
                    out[yy * b.w + x] = 1;
                }
            }
        }
    }
    Bin { w: b.w, h: b.h, px: out }
}

/// Ink pixels in horizontal runs of at least `min_len` (gaps up to `bridge` px are closed).
/// Staff lines are long runs even through note heads and stems; text is not.
fn long_runs(b: &Bin, min_len: usize, bridge: usize) -> Bin {
    let mut out = vec![0u8; b.w * b.h];
    for y in 0..b.h {
        let row = &b.px[y * b.w..(y + 1) * b.w];
        let mut x = 0;
        while x < b.w {
            if row[x] == 0 {
                x += 1;
                continue;
            }
            let s = x;
            let mut e = x; // last ink pixel
            while x < b.w {
                if row[x] != 0 {
                    e = x;
                    x += 1;
                } else if x - e <= bridge {
                    x += 1;
                } else {
                    break;
                }
            }
            if e + 1 - s >= min_len {
                out[y * b.w + s..y * b.w + e + 1].fill(1);
            }
            x = e + 1;
        }
    }
    Bin { w: b.w, h: b.h, px: out }
}

fn skew_score(pts: &[(u16, u16)], w: usize, h: usize, tan: f32, hist: &mut [u32]) -> u64 {
    hist.iter_mut().for_each(|v| *v = 0);
    let off = h as f32 * 0.5;
    for &(x, y) in pts {
        let yy = y as f32 - (x as f32 - w as f32 * 0.5) * tan + off;
        let i = yy as isize;
        if i >= 0 && (i as usize) < hist.len() {
            hist[i as usize] += 1;
        }
    }
    hist.iter().map(|&v| v as u64 * v as u64).sum()
}

/// Skew angle (degrees) that makes line pixels most horizontal.
fn estimate_skew(lines: &Bin) -> f32 {
    let mut pts = Vec::new();
    for y in 0..lines.h {
        for x in 0..lines.w {
            if lines.at(x, y) {
                pts.push((x as u16, y as u16));
            }
        }
    }
    if pts.len() < 1000 {
        return 0.0;
    }
    // Subsample very dense pages.
    if pts.len() > 400_000 {
        let step = pts.len() / 400_000 + 1;
        pts = pts.into_iter().step_by(step).collect();
    }
    let mut hist = vec![0u32; lines.h * 2];
    let eval = |deg: f32, hist: &mut [u32]| skew_score(&pts, lines.w, lines.h, deg.to_radians().tan(), hist);
    let mut best = (0.0f32, eval(0.0, &mut hist));
    let mut deg = -SKEW_MAX_DEG;
    while deg <= SKEW_MAX_DEG {
        let s = eval(deg, &mut hist);
        if s > best.1 {
            best = (deg, s);
        }
        deg += SKEW_COARSE_STEP;
    }
    let c = best.0;
    let mut deg = c - SKEW_COARSE_STEP;
    while deg <= c + SKEW_COARSE_STEP {
        let s = eval(deg, &mut hist);
        if s > best.1 {
            best = (deg, s);
        }
        deg += SKEW_FINE_STEP;
    }
    best.0
}

/// Vertical shear: y' = y - (x - w/2) * tan.
fn shear(b: &Bin, tan: f32) -> Bin {
    if tan == 0.0 {
        return Bin { w: b.w, h: b.h, px: b.px.clone() };
    }
    let mut out = vec![0u8; b.w * b.h];
    for x in 0..b.w {
        let s = ((x as f32 - b.w as f32 * 0.5) * tan).round() as isize;
        for y in 0..b.h {
            let yy = y as isize - s;
            if yy >= 0 && (yy as usize) < b.h && b.at(x, y) {
                out[yy as usize * b.w + x] = 1;
            }
        }
    }
    Bin { w: b.w, h: b.h, px: out }
}

#[derive(Clone, Debug)]
struct Cand {
    y: f32,
    dens: f32,
}

#[derive(Clone, Debug)]
struct Group {
    ys: Vec<f32>,
    g: f32,
    score: f32,
}

/// Line candidates in one strip.
fn strip_candidates(lines: &Bin, x0: usize, x1: usize, fill: f32, tmax: usize) -> Vec<Cand> {
    let sw = (x1 - x0) as f32;
    let mut prof = vec![0u32; lines.h];
    for (y, p) in prof.iter_mut().enumerate() {
        let row = &lines.px[y * lines.w + x0..y * lines.w + x1];
        *p = row.iter().map(|&v| v as u32).sum();
    }
    let thr = (fill * sw) as u32;
    let mut out = Vec::new();
    let mut y = 0;
    while y < lines.h {
        if prof[y] < thr {
            y += 1;
            continue;
        }
        let s = y;
        let (mut sum, mut wsum, mut peak) = (0.0f32, 0.0f32, 0u32);
        while y < lines.h && prof[y] >= thr {
            sum += prof[y] as f32 * y as f32;
            wsum += prof[y] as f32;
            peak = peak.max(prof[y]);
            y += 1;
        }
        if y - s <= 3 * tmax {
            out.push(Cand { y: sum / wsum, dens: peak as f32 / sw });
        }
    }
    out
}

/// All valid 5-line (and 6-line tab) groups among candidates; greedy non-overlapping selection.
fn strip_groups(c: &[Cand], d: f32) -> Vec<Group> {
    let n = c.len();
    let mut all = Vec::new();
    let find = |target: f32, tol: f32| -> Option<usize> {
        let mut best = None;
        for (m, cm) in c.iter().enumerate() {
            let e = (cm.y - target).abs();
            if e <= tol && best.is_none_or(|(_, be)| e < be) {
                best = Some((m, e));
            }
        }
        best.map(|b| b.0)
    };
    for i in 0..n {
        for l in i + 4..n {
            let gg = (c[l].y - c[i].y) / 4.0;
            if gg < (GROUP_MIN_SPACING * d).max(3.0) {
                continue;
            }
            if gg > GROUP_MAX_SPACING * d {
                break;
            }
            let tol = (LINE_TOL * gg).max(1.5);
            // Lines 0 and 4 present; at most one interior line may be missing (dense passages).
            let mut ys = vec![c[i].y];
            let mut dens = vec![c[i].dens];
            let mut missing = 0;
            let mut last = i;
            for k in 1..4 {
                let target = c[i].y + k as f32 * gg;
                match find(target, tol) {
                    Some(m) if m > last && m < l => {
                        ys.push(c[m].y);
                        dens.push(c[m].dens);
                        last = m;
                    }
                    _ => {
                        missing += 1;
                        ys.push(target);
                    }
                }
            }
            if missing > 1 {
                continue;
            }
            ys.push(c[l].y);
            dens.push(c[l].dens);
            let mut sorted = dens.clone();
            sorted.sort_by(f32::total_cmp);
            let med = sorted[sorted.len() / 2];
            if sorted[0] < MIN_LINE_STRENGTH * med {
                continue;
            }
            // Every-other-line patterns across two close staves: candidates in the middle of
            // the gaps mean the real spacing is half as large.
            let between = |a: f32, b: f32| {
                let m = 0.5 * (a + b);
                c.iter().any(|cm| (cm.y - m).abs() < MID_LINE_TOL * gg)
            };
            if ys.windows(2).filter(|p| between(p[0], p[1])).count() >= 2 {
                continue;
            }
            // A missing line costs a full line's density, so complete groups win.
            let score = dens.iter().sum::<f32>() - missing as f32 * med;
            // Tablature: a sixth line of similar density (only for complete groups).
            if missing == 0
                && let Some(m) = find(c[i].y + 5.0 * gg, tol)
                && c[m].dens >= TAB_LINE_STRENGTH * med
                && m > l
            {
                let mut ys6 = ys.clone();
                ys6.push(c[m].y);
                all.push(Group { ys: ys6, g: gg, score: score + c[m].dens });
            }
            all.push(Group { ys, g: gg, score });
        }
    }
    all.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut chosen: Vec<Group> = Vec::new();
    for gr in all {
        let (a0, a1) = (gr.ys[0] - 0.5 * gr.g, *gr.ys.last().unwrap() + 0.5 * gr.g);
        if chosen.iter().all(|o| a1 < o.ys[0] - 0.5 * o.g || a0 > *o.ys.last().unwrap() + 0.5 * o.g) {
            chosen.push(gr);
        }
    }
    chosen
}

struct Track {
    lines: usize,
    last_strip: usize,
    last_top: f32,
    g: f32,
    members: Vec<(usize, Group)>,
}

/// Detects staves, systems and cut lines on one page.
pub fn detect(gray: &Gray) -> PageResult {
    let (w, h) = (gray.w, gray.h);
    let thr = otsu(gray);
    let bin = Bin { w, h, px: gray.px.iter().map(|&v| (v < thr) as u8).collect() };
    let ink_total: usize = bin.px.iter().map(|&v| v as usize).sum();
    let (t, d0) = run_lengths(&bin);
    let tmax = (THIN_RUN_FACTOR * t + 1).max(t + THIN_RUN_EXTRA);
    let lines0 = line_pixels(&bin, tmax);
    let skew = estimate_skew(&lines0);
    let tan = if skew.abs() < SKEW_IGNORE_DEG { 0.0 } else { skew.to_radians().tan() };
    let ink = shear(&bin, tan);
    drop(lines0);
    let d = d0 as f32;
    let lines = long_runs(&ink, ((LONG_RUN_SPACES * d) as usize).max(LONG_RUN_MIN_PX), LONG_RUN_BRIDGE_PX);
    let r = (t as f32 * 0.5).ceil() as isize + 1;

    // 5. Strips.
    let sw = ((STRIP_SPACES * d) as usize).clamp(32, (w / 4).max(32));
    let nstrips = w / sw;
    let mut per_strip: Vec<Vec<Group>> = Vec::with_capacity(nstrips);
    for k in 0..nstrips {
        let c = strip_candidates(&lines, k * sw, (k + 1) * sw, MIN_LINE_FILL, tmax);
        let gr = strip_groups(&c, d);
        per_strip.push(gr);
    }

    // 6. Link into tracks.
    let mut tracks: Vec<Track> = Vec::new();
    for (k, groups) in per_strip.into_iter().enumerate() {
        let mut taken = vec![false; tracks.len()];
        for gr in groups {
            let top = gr.ys[0];
            let mut best: Option<(usize, f32)> = None;
            for (ti, tr) in tracks.iter().enumerate() {
                if taken[ti] || tr.last_strip + TRACK_MAX_GAP_STRIPS < k || tr.lines != gr.ys.len() {
                    continue;
                }
                let e = (tr.last_top - top).abs();
                if e < TRACK_Y_TOL * tr.g
                    && (tr.g - gr.g).abs() < TRACK_SPACING_TOL * tr.g
                    && best.is_none_or(|(_, be)| e < be)
                {
                    best = Some((ti, e));
                }
            }
            match best {
                Some((ti, _)) => {
                    let tr = &mut tracks[ti];
                    tr.last_strip = k;
                    tr.last_top = top;
                    tr.members.push((k, gr));
                    taken[ti] = true;
                }
                None => {
                    tracks.push(Track {
                        lines: gr.ys.len(),
                        last_strip: k,
                        last_top: top,
                        g: gr.g,
                        members: vec![(k, gr)],
                    });
                    taken.push(true);
                }
            }
        }
    }
    // Merge tracks of the same staff broken by dense passages.
    tracks.sort_by(|a, b| a.members[0].1.ys[0].total_cmp(&b.members[0].1.ys[0]));
    let median_top = |tr: &Track| {
        let mut v: Vec<f32> = tr.members.iter().map(|m| m.1.ys[0]).collect();
        v.sort_by(f32::total_cmp);
        v[v.len() / 2]
    };
    let mut merged: Vec<Track> = Vec::new();
    for tr in tracks {
        let top = median_top(&tr);
        if let Some(o) = merged.iter_mut().find(|o| {
            o.lines == tr.lines
                && (median_top(o) - top).abs() < TRACK_Y_TOL * o.g
                && tr.members.iter().all(|m| o.members.iter().all(|om| om.0 != m.0))
        }) {
            o.members.extend(tr.members);
            o.members.sort_by_key(|m| m.0);
        } else {
            merged.push(tr);
        }
    }
    let mut staves: Vec<Staff> = Vec::new();
    for tr in merged {
        let n = tr.lines;
        let first = tr.members[0].0;
        let last = tr.members[tr.members.len() - 1].0;
        let span = ((last - first + 1) * sw) as f32;
        if span < MIN_STAFF_SPACES * d || tr.members.len() < 2 {
            continue;
        }
        let mut line_y = Vec::with_capacity(n);
        for i in 0..n {
            let mut v: Vec<f32> = tr.members.iter().map(|m| m.1.ys[i]).collect();
            v.sort_by(f32::total_cmp);
            line_y.push(v[v.len() / 2]);
        }
        let space = (line_y[n - 1] - line_y[0]) / (n - 1) as f32;
        let samples = tr.members.iter().map(|m| ((m.0 * sw + sw / 2) as f32, m.1.ys[0])).collect();
        staves.push(Staff {
            lines: n,
            line_y,
            space,
            x0: (first * sw) as f32,
            x1: ((last + 1) * sw) as f32,
            strips: tr.members.len(),
            samples,
        });
    }
    // Overlapping staves (e.g. 5-line view of a tab staff): keep the better supported one.
    staves.sort_by_key(|s| std::cmp::Reverse(s.strips));
    let mut kept: Vec<Staff> = Vec::new();
    for s in staves {
        if kept.iter().all(|o| s.bottom() + 0.5 * s.space < o.top() || s.top() - 0.5 * s.space > o.bottom()) {
            kept.push(s);
        }
    }
    let mut staves = kept;
    // Six lines are only tablature if the spacing is clearly wider than the page's normal
    // staves; otherwise it is a staff plus a ledger/neighbouring line: drop the weaker outer line.
    let mut sp5: Vec<f32> = staves.iter().filter(|s| s.lines == 5).map(|s| s.space).collect();
    sp5.sort_by(f32::total_cmp);
    if let Some(&med5) = sp5.get(sp5.len() / 2) {
        for s in staves.iter_mut().filter(|s| s.lines == 6 && s.space < TAB_MIN_SPACING * med5) {
            let cover = |y: f32| -> usize {
                let yy = y.round() as usize;
                (s.x0 as usize..(s.x1 as usize).min(w)).filter(|&x| lines.at(x, yy.min(h - 1))).count()
            };
            if cover(s.line_y[0]) < cover(s.line_y[5]) {
                s.line_y.remove(0);
            } else {
                s.line_y.pop();
            }
            s.lines = 5;
        }
    }
    staves.sort_by(|a, b| a.top().total_cmp(&b.top()));

    // Refine horizontal extent at pixel level on the deskewed ink image.
    for s in staves.iter_mut() {
        let count = |x: usize, s: &Staff| -> usize {
            let off = s.offset_at(x as f32);
            s.line_y
                .iter()
                .filter(|&&ly| {
                    let c = (ly + off).round() as isize;
                    (c - r..=c + r).any(|y| y >= 0 && (y as usize) < h && ink.at(x, y as usize))
                })
                .count()
        };
        let need = s.lines - 1;
        let gap = (EXTENT_GAP_SPACES * s.space) as usize + 1;
        let mut x = s.x0 as usize + sw / 2;
        let mut good = x;
        while x > 0 && good - x <= gap {
            x -= 1;
            if count(x, s) >= need {
                good = x;
            }
        }
        let new_x0 = good;
        let mut x = (s.x1 as usize).saturating_sub(sw / 2).min(w - 1);
        let mut good = x;
        while x + 1 < w && x - good <= gap {
            x += 1;
            if count(x, s) >= need {
                good = x;
            }
        }
        s.x0 = new_x0 as f32;
        s.x1 = good as f32;
    }

    // 7. Group staves into systems via a left-edge connection.
    let row_cover = |y0: isize, y1: isize, x0: f32, x1: f32| -> f32 {
        let (x0, x1) = (x0.max(0.0) as usize, (x1 as usize).min(w));
        if y1 <= y0 || x1 <= x0 {
            return 1.0;
        }
        let mut n = 0;
        for y in y0.max(0)..y1.min(h as isize) {
            if (x0..x1).any(|x| ink.at(x, y as usize)) {
                n += 1;
            }
        }
        n as f32 / (y1 - y0) as f32
    };
    let mut systems: Vec<Vec<usize>> = Vec::new();
    for i in 0..staves.len() {
        let connected = i > 0 && {
            let (a, b) = (&staves[i - 1], &staves[i]);
            let l = a.x0.min(b.x0);
            let dd = a.space.max(b.space);
            let cov = row_cover(
                a.bottom() as isize + r + 1,
                b.top() as isize - r,
                l - CONNECT_LEFT_SPACES * dd,
                l + CONNECT_RIGHT_SPACES * dd,
            );
            // Barlines anywhere (e.g. after an incipit, or group barlines) also connect: a
            // narrow column fully inked across the gap. Stems are shorter than such gaps.
            cov >= MIN_CONNECT || {
                let (y0, y1) = (a.bottom() as isize + r + 1, b.top() as isize - r);
                let (xa, xb) = (a.x0.max(b.x0) as usize, (a.x1.min(b.x1) as usize).min(w));
                let half = (t / 2 + 1).max(1);
                y1 - y0 > 2
                    && (xa + half..xb.saturating_sub(half)).any(|x| {
                        let n = (y0..y1).filter(|&y| (x - half..=x + half).any(|xx| ink.at(xx, y as usize))).count();
                        n as f32 >= BARLINE_COVER * (y1 - y0) as f32
                    })
            }
        };
        if connected {
            systems.last_mut().expect("a system exists").push(i);
        } else {
            systems.push(vec![i]);
        }
    }

    // 8. Cuts.
    let mut issues = Vec::new();
    let (cx0, cx1) = if staves.is_empty() {
        (0.0, w as f32)
    } else {
        (
            (staves.iter().map(|s| s.x0).fold(f32::MAX, f32::min) - CONTENT_MARGIN_SPACES * d).max(0.0),
            (staves.iter().map(|s| s.x1).fold(0.0, f32::max) + CONTENT_MARGIN_SPACES * d).min(w as f32),
        )
    };
    let prof: Vec<u32> =
        (0..h).map(|y| ink.px[y * w + cx0 as usize..y * w + cx1 as usize].iter().map(|&v| v as u32).sum()).collect();
    let noise = ((cx1 - cx0) * NOISE_FRACTION).max(1.0) as u32;
    // Scanner borders: rows at the top/bottom edge that are mostly ink count as page edge.
    let full_row = |y: usize| ink.px[y * w..(y + 1) * w].iter().filter(|&&v| v != 0).count();
    let mut page_top = 0;
    let border = |y: usize| full_row(y) as f32 > BORDER_ROW_FILL * w as f32;
    while page_top < h / 4 && border(page_top) {
        page_top += 1;
    }
    let mut page_bot = h;
    while page_bot > h * 3 / 4 && border(page_bot - 1) {
        page_bot -= 1;
    }
    // Longest ink-free run in [a, b) → (cut row, clean).
    let cut_in = |a: isize, b: isize| -> (f32, bool) {
        let (a, b) = (a.max(0) as usize, (b.max(0) as usize).min(h));
        if b <= a {
            return (a as f32, false);
        }
        let mid = (a + b) as f32 * 0.5;
        let mut best: Option<(usize, usize)> = None;
        let mut y = a;
        while y < b {
            if prof[y] > noise {
                y += 1;
                continue;
            }
            let s = y;
            while y < b && prof[y] <= noise {
                y += 1;
            }
            let better = match best {
                None => true,
                Some((bs, be)) => {
                    let (l, bl) = (y - s, be - bs);
                    l > bl || (l == bl && ((s + y) as f32 * 0.5 - mid).abs() < ((bs + be) as f32 * 0.5 - mid).abs())
                }
            };
            if better {
                best = Some((s, y));
            }
        }
        match best {
            Some((s, e)) => ((s + e) as f32 * 0.5, true),
            None => {
                let mut bi = a;
                for y in a..b {
                    let sm = |y: usize| prof[y.saturating_sub(1)] + prof[y] + prof[(y + 1).min(h - 1)];
                    if sm(y) < sm(bi) || (sm(y) == sm(bi) && (y as f32 - mid).abs() < (bi as f32 - mid).abs()) {
                        bi = y;
                    }
                }
                // Cutting through a few pixels (slur tip, hairpin end) is harmless; only
                // flag cuts that cross real ink (more than 2 % of the content width).
                (bi as f32, prof[bi] as f32 <= CUT_INK_OK * (cx1 - cx0))
            }
        }
    };
    let sys_top = |sy: &Vec<usize>| staves[sy[0]].top();
    let sys_bot = |sy: &Vec<usize>| staves[sy[sy.len() - 1]].bottom();
    let mut gaps: Vec<f32> = systems.windows(2).map(|p| sys_top(&p[1]) - sys_bot(&p[0])).collect();
    gaps.sort_by(f32::total_cmp);
    let typ_gap = gaps.get(gaps.len() / 2).copied().unwrap_or(DEFAULT_GAP_SPACES * d);
    let margin_win = (MARGIN_GAP_FACTOR * typ_gap).max(MARGIN_MIN_SPACES * d);
    // Above the first / below the last system: ink-free run scored by length and closeness to
    // the staff, so a big gap before a title/footer wins over small gaps inside the system's
    // own markings (tempo, 8vb, fingering).
    let cut_margin = |a: isize, b: isize, anchor: f32| -> (f32, bool) {
        let (a, b) = (a.max(0) as usize, (b.max(0) as usize).min(h));
        let mut best: Option<(f32, f32)> = None;
        let mut y = a;
        while y < b {
            if prof[y] > noise {
                y += 1;
                continue;
            }
            let s = y;
            while y < b && prof[y] <= noise {
                y += 1;
            }
            let dist = (s as f32 - anchor).abs().min((y as f32 - anchor).abs());
            // A run touching the page edge is the margin, not a separator: it only wins if
            // there is no other run (then there is no header/footer ink at all).
            let edge = s <= page_top || y >= page_bot;
            let score = if edge { -1.0 } else { (y - s) as f32 / (1.0 + dist / (MARGIN_DECAY_SPACES * d)) };
            let c = if !edge {
                (s + y) as f32 * 0.5
            } else if s == 0 {
                (y as f32 - EDGE_CUT_SPACES * d).max(0.0)
            } else {
                (s as f32 + EDGE_CUT_SPACES * d).min(h as f32)
            };
            if best.is_none_or(|(bs, _)| score > bs) {
                best = Some((score, c));
            }
        }
        match best {
            Some((_, c)) => (c, true),
            None if anchor > a as f32 => cut_in((anchor - margin_win) as isize, b as isize),
            None => cut_in(a as isize, (anchor + margin_win) as isize),
        }
    };
    let mut cuts: Vec<(f32, bool)> = Vec::new(); // cut above system k (and below last)
    let ri = |v: f32| v as isize;
    for (k, sy) in systems.iter().enumerate() {
        let top = sys_top(sy);
        if k == 0 {
            cuts.push(cut_margin(0, ri(top) - 2 * r, top));
        } else {
            cuts.push(cut_in(ri(sys_bot(&systems[k - 1])) + 2 * r, ri(top) - 2 * r));
        }
    }
    if let Some(last) = systems.last() {
        let b = sys_bot(last);
        let mut cut = cut_margin(ri(b) + 2 * r, h as isize, b);
        // Ink right below the last system (8vb line, low notes) belongs to it: move the cut
        // below that ink block (ended by an ink-free run of one space).
        let first_ink = ((cut.0.max(0.0) as usize)..page_bot.min(h)).find(|&y| prof[y] > noise);
        if let Some(y0) = first_ink
            && (y0 as f32) - b < FOOTER_ATTACH_SPACES * d
        {
            let (mut end, mut white, mut y) = (y0 + 1, 0usize, y0);
            while y < page_bot.min(h) && (white as f32) < d {
                if prof[y] > noise {
                    end = y + 1;
                    white = 0;
                } else {
                    white += 1;
                }
                y += 1;
            }
            cut = ((end as f32 + y as f32) * 0.5, true);
        }
        cuts.push(cut);
    }

    // Regions in deskewed px, then mapped back to the page.
    let ink_rows = |y0: f32, y1: f32| -> Option<(f32, f32)> {
        let (a, b) = (y0.max(0.0) as usize, (y1 as usize).min(h));
        let first = (a..b).find(|&y| prof[y] > noise)?;
        let last = (a..b).rev().find(|&y| prof[y] > noise)?;
        Some((first as f32, last as f32 + 1.0))
    };
    let ink_cols = |y0: f32, y1: f32, lo: f32, hi: f32| -> (f32, f32) {
        let (a, b) = (y0.max(0.0) as usize, (y1 as usize).min(h));
        let (lo, hi) = (lo.max(0.0) as usize, (hi as usize).min(w));
        let min_n = ((b - a) as f32 * INK_COLUMN_FILL).max(1.0) as usize;
        let col = |x: usize| (a..b).filter(|&y| ink.at(x, y)).count();
        let x0 = (lo..hi).find(|&x| col(x) > min_n).unwrap_or(lo);
        let x1 = (lo..hi).rev().find(|&x| col(x) > min_n).unwrap_or(hi);
        (x0 as f32, x1 as f32 + 1.0)
    };
    let half_w = w as f32 * 0.5;
    let to_page = |kind: Kind, y0: f32, y1: f32, iy: (f32, f32), x0: f32, x1: f32| -> Region {
        let s0 = (x0 - half_w) * tan;
        let s1 = (x1 - half_w) * tan;
        let (smin, smax) = (s0.min(s1), s0.max(s1));
        let ny = |v: f32| (v / h as f32).clamp(0.0, 1.0);
        Region {
            kind,
            y0: ny(y0 + smin),
            y1: ny(y1 + smax),
            ink_y0: ny(iy.0 + smin),
            ink_y1: ny(iy.1 + smax),
            x0: (x0 / w as f32).clamp(0.0, 1.0),
            x1: (x1 / w as f32).clamp(0.0, 1.0),
            staves: 0,
            staff_space: 0.0,
            clean_top: true,
            clean_bottom: true,
        }
    };
    let mut regions = Vec::new();
    if !systems.is_empty() {
        let c0 = cuts[0].0;
        if let Some(iy) = ink_rows(0.0, c0) {
            let (x0, x1) = ink_cols(iy.0, iy.1, 0.0, w as f32);
            regions.push(to_page(Kind::Header, iy.0, c0, iy, x0, x1));
        }
        for (k, sy) in systems.iter().enumerate() {
            let (y0, y1) = (cuts[k].0, cuts[k + 1].0);
            let sx0 = sy.iter().map(|&i| staves[i].x0).fold(f32::MAX, f32::min);
            let sx1 = sy.iter().map(|&i| staves[i].x1).fold(0.0, f32::max);
            let dd = staves[sy[0]].space;
            let (x0, x1) = ink_cols(y0, y1, sx0 - SYSTEM_LEFT_SPACES * dd, sx1 + SYSTEM_RIGHT_SPACES * dd);
            let iy = ink_rows(y0, y1).unwrap_or((y0, y1));
            let mut reg = to_page(Kind::System, y0, y1, iy, x0, x1);
            reg.staves = sy.len();
            let mut sp: Vec<f32> = sy.iter().map(|&i| staves[i].space).collect();
            sp.sort_by(f32::total_cmp);
            reg.staff_space = sp[0] / h as f32;
            reg.clean_top = cuts[k].1;
            reg.clean_bottom = cuts[k + 1].1;
            regions.push(reg);
        }
        let cl = cuts[cuts.len() - 1].0;
        if let Some(iy) = ink_rows(cl, h as f32) {
            let (x0, x1) = ink_cols(iy.0, iy.1, 0.0, w as f32);
            regions.push(to_page(Kind::Footer, cl, iy.1, iy, x0, x1));
        }
    }

    // 9. Confidence.
    let mut conf = 1.0f32;
    let ink_frac = ink_total as f32 / (w * h) as f32;
    if staves.is_empty() {
        if ink_frac > BLANK_INK {
            conf = NO_STAFF_CONFIDENCE;
            issues.push("no staves found".into());
        } else {
            issues.push("blank page".into());
        }
    }
    let unclean = regions.iter().filter(|r| r.kind == Kind::System && !r.clean_bottom).count()
        + regions.iter().filter(|r| r.kind == Kind::System).take(1).filter(|r| !r.clean_top).count();
    if unclean > 0 {
        conf *= PENALTY_UNCLEAN_CUT.powi(unclean.min(3) as i32);
        issues.push(format!("{unclean} cut(s) through ink"));
    }
    // Inside a system region, far from its staves: several line-like rows (long horizontal
    // runs over a quarter of the content width) mean a staff was probably missed; plain ink
    // (titles, footnotes, lyrics) only lowers confidence a little.
    for (k, sy) in systems.iter().enumerate() {
        let (y0, y1) = (cuts[k].0 as usize, (cuts[k + 1].0 as usize).min(h));
        let near = |y: usize| {
            sy.iter().any(|&i| {
                let s = &staves[i];
                y as f32 >= s.top() - NEAR_STAFF_SPACES * s.space
                    && y as f32 <= s.bottom() + NEAR_STAFF_SPACES * s.space
            })
        };
        let (lx0, lx1) = (cx0 as usize, cx1 as usize);
        let line_min = ((cx1 - cx0) * MISSED_LINE_FILL) as usize;
        let heavy = ((cx1 - cx0) * FAR_INK_FILL) as u32;
        let (mut line_rows, mut in_line, mut run, mut far_ink) = (0, false, 0, false);
        for (y, &ink_in_row) in prof.iter().enumerate().take(y1).skip(y0) {
            if near(y) {
                in_line = false;
                run = 0;
                continue;
            }
            let cov = lines.px[y * w + lx0..y * w + lx1].iter().filter(|&&v| v != 0).count();
            if cov >= line_min && !in_line {
                line_rows += 1;
            }
            in_line = cov >= line_min;
            run = if ink_in_row > heavy { run + 1 } else { 0 };
            far_ink |= run as f32 > 2.0 * d;
        }
        if line_rows >= 3 {
            conf *= PENALTY_MISSED_STAFF;
            issues.push(format!("system {}: staff-like lines far from its staves (missed staff?)", k + 1));
        } else if far_ink {
            conf *= PENALTY_FAR_INK;
            issues.push(format!("system {}: text/ink far from its staves", k + 1));
        }
    }
    // Inconsistent staff counts per system.
    if systems.len() >= 2 {
        let mut counts: Vec<usize> = systems.iter().map(|s| s.len()).collect();
        counts.sort();
        let mode = counts[counts.len() / 2];
        let odd = systems.iter().filter(|s| s.len() != mode).count();
        if odd > 0 {
            conf *= PENALTY_STAFF_COUNT.powi(odd.min(3) as i32);
            issues.push(format!("{odd} system(s) with staff count != {mode}"));
        }
    }
    // Staves in one system should have similar widths.
    for (k, sy) in systems.iter().enumerate() {
        let ws: Vec<f32> = sy.iter().map(|&i| staves[i].x1 - staves[i].x0).collect();
        let mx = ws.iter().cloned().fold(0.0, f32::max);
        if ws.iter().any(|&v| v < MIN_WIDTH_RATIO * mx) {
            conf *= PENALTY_WIDTHS;
            issues.push(format!("system {}: staff widths differ", k + 1));
        }
    }
    if !staves.is_empty() {
        let mut sp: Vec<f32> = staves.iter().filter(|s| s.lines == 5).map(|s| s.space).collect();
        sp.sort_by(f32::total_cmp);
        let med = sp.get(sp.len() / 2).copied().unwrap_or(0.0);
        let odd = staves.iter().filter(|s| s.lines == 5 && (s.space - med).abs() > SPACING_DEVIATION * med).count();
        if odd > 0 {
            conf *= PENALTY_SPACING;
            issues.push(format!("{odd} staff/staves with unusual line distance"));
        }
        if regions.iter().filter(|r| r.kind == Kind::System).any(|r| r.y1 - r.y0 > 0.9) {
            issues.push("one system fills the page".into());
        }
    }
    if skew.abs() > SKEW_WARN_DEG {
        conf *= PENALTY_SKEW;
        issues.push(format!("skew {skew:.2} deg"));
    }

    PageResult {
        width_px: w,
        height_px: h,
        skew_deg: skew,
        line_thickness_px: t as f32,
        line_distance_px: d,
        systems: systems.len(),
        staves,
        regions,
        confidence: conf,
        issues,
    }
}
