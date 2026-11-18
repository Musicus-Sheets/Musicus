//! Music-system detection on a greyscale page raster.
//!
//! Pipeline (all steps linear in the number of pixels, no allocation per pixel):
//! 1. Otsu binarisation.
//! 2. Vertical run-length histograms → staff-line thickness `t` and line distance `d`.
//! 3. "Line pixels" = ink in vertical runs no longer than ~2t (removes stems, heads, text bodies).
//! 4. Skew estimate (max. sharpness of the line-pixel row projection over ±3°), vertical shear to deskew.
//! 5. Per vertical strip (~8 d wide): row projection of line pixels → line candidates →
//!    groups of 5 (or 6 = tablature) equally spaced lines.
//! 6. Link strip candidates left→right into staves; merge broken tracks; drop short ones.
//! 7. Staves connected at their left end (system barline / brace / bracket) form a system.
//! 8. Cuts between systems at the longest ink-free row run (else minimum ink); header above the
//!    first system, footer below the last.
//! 9. Confidence from consistency checks; pages below the threshold fall back to full-page view.

use crate::raster::Gray;
use serde::Serialize;

#[derive(Clone, Debug)]
pub struct Params {
    /// Strip width in line distances.
    pub strip_spaces: f32,
    /// Minimum fraction of a strip row covered by line pixels for a line candidate.
    pub min_line_fill: f32,
    /// Minimum row coverage of the left-edge window for two staves to be connected.
    pub min_connect: f32,
    /// Pages below this confidence fall back to full-page view.
    pub confidence_threshold: f32,
}

impl Default for Params {
    fn default() -> Self {
        Params { strip_spaces: 8.0, min_line_fill: 0.50, min_connect: 0.90, confidence_threshold: 0.6 }
    }
}

/// A staff in deskewed pixel coordinates.
#[derive(Serialize, Clone, Debug)]
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
    #[serde(skip)]
    samples: Vec<(f32, f32)>,
}

impl Staff {
    pub fn top(&self) -> f32 {
        self.line_y[0]
    }
    pub fn bottom(&self) -> f32 {
        *self.line_y.last().unwrap()
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

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Header,
    System,
    Footer,
}

/// A region of the page. Coordinates are page-relative (0..1) in the original (not deskewed)
/// page; with skew they are the bounding box of the slanted region.
#[derive(Serialize, Clone, Debug)]
pub struct Region {
    pub kind: Kind,
    /// Cut lines above/below.
    pub y0: f32,
    pub y1: f32,
    /// Ink extent inside the cut lines (what a reader would show, plus padding).
    pub ink_y0: f32,
    pub ink_y1: f32,
    pub x0: f32,
    pub x1: f32,
    pub staves: usize,
    pub staff_lines: Vec<usize>,
    /// Line distance relative to the page height (0 for header/footer).
    pub staff_space: f32,
    /// Cut above/below went through ink-free rows.
    pub clean_top: bool,
    pub clean_bottom: bool,
    /// Deskewed px coordinates (for drawing).
    #[serde(skip)]
    pub d: [f32; 4],
}

#[derive(Serialize, Clone, Debug)]
pub struct PageResult {
    pub width_px: usize,
    pub height_px: usize,
    pub threshold: u8,
    pub skew_deg: f32,
    pub line_thickness_px: f32,
    pub line_distance_px: f32,
    pub staves: Vec<Staff>,
    pub regions: Vec<Region>,
    pub systems: usize,
    pub confidence: f32,
    pub fallback: bool,
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
    for t in 0..256 {
        wb += hist[t] as f64;
        if wb == 0.0 {
            continue;
        }
        let wf = total - wb;
        if wf == 0.0 {
            break;
        }
        sb += t as f64 * hist[t] as f64;
        let mb = sb / wb;
        let mf = (sum - sb) / wf;
        let between = wb * wf * (mb - mf) * (mb - mf);
        if between > best {
            best = between;
            thr = t as u8;
        }
    }
    thr.clamp(60, 210)
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
    let mut deg = -3.0;
    while deg <= 3.0 {
        let s = eval(deg, &mut hist);
        if s > best.1 {
            best = (deg, s);
        }
        deg += 0.1;
    }
    let c = best.0;
    let mut deg = c - 0.1;
    while deg <= c + 0.1 {
        let s = eval(deg, &mut hist);
        if s > best.1 {
            best = (deg, s);
        }
        deg += 0.01;
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
            if gg < (0.4 * d).max(3.0) {
                continue;
            }
            if gg > 2.5 * d {
                break;
            }
            let tol = (0.2 * gg).max(1.5);
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
            if sorted[0] < 0.45 * med {
                continue;
            }
            // Every-other-line patterns across two close staves: candidates in the middle of
            // the gaps mean the real spacing is half as large.
            let between = |a: f32, b: f32| {
                let m = 0.5 * (a + b);
                c.iter().any(|cm| (cm.y - m).abs() < 0.2 * gg)
            };
            if ys.windows(2).filter(|p| between(p[0], p[1])).count() >= 2 {
                continue;
            }
            // A missing line costs a full line's density, so complete groups win.
            let score = dens.iter().sum::<f32>() - missing as f32 * med;
            // Tablature: a sixth line of similar density (only for complete groups).
            if missing == 0
                && let Some(m) = find(c[i].y + 5.0 * gg, tol)
                && c[m].dens >= 0.7 * med
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

pub fn detect(gray: &Gray, p: &Params) -> PageResult {
    let (w, h) = (gray.w, gray.h);
    let thr = otsu(gray);
    let bin = Bin { w, h, px: gray.px.iter().map(|&v| (v < thr) as u8).collect() };
    let ink_total: usize = bin.px.iter().map(|&v| v as usize).sum();
    let (t, d0) = run_lengths(&bin);
    let tmax = (2 * t + 1).max(t + 3);
    let lines0 = line_pixels(&bin, tmax);
    let skew = estimate_skew(&lines0);
    let tan = if skew.abs() < 0.02 { 0.0 } else { skew.to_radians().tan() };
    let ink = shear(&bin, tan);
    drop(lines0);
    let d = d0 as f32;
    let lines = long_runs(&ink, ((1.5 * d) as usize).max(8), 1);
    let r = (t as f32 * 0.5).ceil() as isize + 1;

    // 5. Strips.
    let sw = ((p.strip_spaces * d) as usize).clamp(32, (w / 4).max(32));
    let nstrips = w / sw;
    let mut per_strip: Vec<Vec<Group>> = Vec::with_capacity(nstrips);
    for k in 0..nstrips {
        let c = strip_candidates(&lines, k * sw, (k + 1) * sw, p.min_line_fill, tmax);
        let gr = strip_groups(&c, d);
        if std::env::var_os("SYSDETECT_DEBUG").is_some() {
            eprintln!(
                "strip {k}: cands {:?}\n   groups {:?}",
                c.iter().map(|c| (c.y.round() as i32, (c.dens * 100.0) as i32)).collect::<Vec<_>>(),
                gr.iter().map(|g| (g.ys[0].round() as i32, g.ys.len(), g.g)).collect::<Vec<_>>()
            );
        }
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
                if taken[ti] || tr.last_strip + 4 < k || tr.lines != gr.ys.len() {
                    continue;
                }
                let e = (tr.last_top - top).abs();
                if e < 0.6 * tr.g && (tr.g - gr.g).abs() < 0.2 * tr.g && best.is_none_or(|(_, be)| e < be) {
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
                && (median_top(o) - top).abs() < 0.6 * o.g
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
        let first = tr.members.first().unwrap().0;
        let last = tr.members.last().unwrap().0;
        let span = ((last - first + 1) * sw) as f32;
        if span < 12.0 * d || tr.members.len() < 2 {
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
    staves.sort_by(|a, b| b.strips.cmp(&a.strips));
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
        for s in staves.iter_mut().filter(|s| s.lines == 6 && s.space < 1.25 * med5) {
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
        let gap = (0.6 * s.space) as usize + 1;
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
            let cov = row_cover(a.bottom() as isize + r + 1, b.top() as isize - r, l - 3.0 * dd, l + 1.0 * dd);
            // Barlines anywhere (e.g. after an incipit, or group barlines) also connect: a
            // narrow column fully inked across the gap. Stems are shorter than such gaps.
            cov >= p.min_connect || {
                let (y0, y1) = (a.bottom() as isize + r + 1, b.top() as isize - r);
                let (xa, xb) = (a.x0.max(b.x0) as usize, (a.x1.min(b.x1) as usize).min(w));
                let half = (t / 2 + 1).max(1);
                y1 - y0 > 2
                    && (xa + half..xb.saturating_sub(half)).any(|x| {
                        let n = (y0..y1).filter(|&y| (x - half..=x + half).any(|xx| ink.at(xx, y as usize))).count();
                        n as f32 >= 0.97 * (y1 - y0) as f32
                    })
            }
        };
        if connected {
            systems.last_mut().unwrap().push(i);
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
            (staves.iter().map(|s| s.x0).fold(f32::MAX, f32::min) - 2.0 * d).max(0.0),
            (staves.iter().map(|s| s.x1).fold(0.0, f32::max) + 2.0 * d).min(w as f32),
        )
    };
    let prof: Vec<u32> =
        (0..h).map(|y| ink.px[y * w + cx0 as usize..y * w + cx1 as usize].iter().map(|&v| v as u32).sum()).collect();
    let noise = ((cx1 - cx0) * 0.004).max(1.0) as u32;
    // Scanner borders: rows at the top/bottom edge that are mostly ink count as page edge.
    let full_row = |y: usize| ink.px[y * w..(y + 1) * w].iter().filter(|&&v| v != 0).count();
    let mut page_top = 0;
    while page_top < h / 4 && full_row(page_top) * 2 > w {
        page_top += 1;
    }
    let mut page_bot = h;
    while page_bot > h * 3 / 4 && full_row(page_bot - 1) * 2 > w {
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
                (bi as f32, prof[bi] as f32 <= 0.02 * (cx1 - cx0))
            }
        }
    };
    let sys_top = |sy: &Vec<usize>| staves[sy[0]].top();
    let sys_bot = |sy: &Vec<usize>| staves[*sy.last().unwrap()].bottom();
    let mut gaps: Vec<f32> = systems.windows(2).map(|p| sys_top(&p[1]) - sys_bot(&p[0])).collect();
    gaps.sort_by(f32::total_cmp);
    let typ_gap = gaps.get(gaps.len() / 2).copied().unwrap_or(8.0 * d);
    let margin_win = (0.6 * typ_gap).max(5.0 * d);
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
            let score = if edge { -1.0 } else { (y - s) as f32 / (1.0 + dist / (8.0 * d)) };
            let c = if !edge {
                (s + y) as f32 * 0.5
            } else if s == 0 {
                (y as f32 - 2.0 * d).max(0.0)
            } else {
                (s as f32 + 2.0 * d).min(h as f32)
            };
            if std::env::var_os("SYSDETECT_DEBUG").is_some() {
                eprintln!("margin run {s}..{y} anchor {anchor} score {score:.1} (page {page_top}..{page_bot})");
            }
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
        cuts.push(cut_margin(ri(b) + 2 * r, h as isize, b));
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
        let min_n = ((b - a) as f32 * 0.002).max(1.0) as usize;
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
            staff_lines: vec![],
            staff_space: 0.0,
            clean_top: true,
            clean_bottom: true,
            d: [x0, y0, x1, y1],
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
            let (x0, x1) = ink_cols(y0, y1, sx0 - 12.0 * dd, sx1 + 2.0 * dd);
            let iy = ink_rows(y0, y1).unwrap_or((y0, y1));
            let mut reg = to_page(Kind::System, y0, y1, iy, x0, x1);
            reg.staves = sy.len();
            reg.staff_lines = sy.iter().map(|&i| staves[i].lines).collect();
            let mut sp: Vec<f32> = sy.iter().map(|&i| staves[i].space).collect();
            sp.sort_by(f32::total_cmp);
            reg.staff_space = sp[0] / h as f32;
            reg.clean_top = cuts[k].1;
            reg.clean_bottom = cuts[k + 1].1;
            regions.push(reg);
        }
        let cl = cuts.last().unwrap().0;
        if let Some(iy) = ink_rows(cl, h as f32) {
            let (x0, x1) = ink_cols(iy.0, iy.1, 0.0, w as f32);
            regions.push(to_page(Kind::Footer, cl, iy.1, iy, x0, x1));
        }
    }

    // 9. Confidence.
    let mut conf = 1.0f32;
    let ink_frac = ink_total as f32 / (w * h) as f32;
    if staves.is_empty() {
        if ink_frac > 0.003 {
            conf = 0.2;
            issues.push("no staves found".into());
        } else {
            issues.push("blank page".into());
        }
    }
    let unclean = regions.iter().filter(|r| r.kind == Kind::System && !r.clean_bottom).count()
        + regions.iter().filter(|r| r.kind == Kind::System).take(1).filter(|r| !r.clean_top).count();
    if unclean > 0 {
        conf *= 0.85f32.powi(unclean.min(3) as i32);
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
                y as f32 >= s.top() - 7.0 * s.space && y as f32 <= s.bottom() + 7.0 * s.space
            })
        };
        let (lx0, lx1) = (cx0 as usize, cx1 as usize);
        let line_min = ((cx1 - cx0) * 0.25) as usize;
        let heavy = ((cx1 - cx0) * 0.05) as u32;
        let (mut line_rows, mut in_line, mut run, mut far_ink) = (0, false, 0, false);
        for y in y0..y1 {
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
            run = if prof[y] > heavy { run + 1 } else { 0 };
            far_ink |= run as f32 > 2.0 * d;
        }
        if line_rows >= 3 {
            conf *= 0.5;
            issues.push(format!("system {}: staff-like lines far from its staves (missed staff?)", k + 1));
        } else if far_ink {
            conf *= 0.9;
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
            conf *= 0.8f32.powi(odd.min(3) as i32);
            issues.push(format!("{odd} system(s) with staff count != {mode}"));
        }
    }
    // Staves in one system should have similar widths.
    for (k, sy) in systems.iter().enumerate() {
        let ws: Vec<f32> = sy.iter().map(|&i| staves[i].x1 - staves[i].x0).collect();
        let mx = ws.iter().cloned().fold(0.0, f32::max);
        if ws.iter().any(|&v| v < 0.85 * mx) {
            conf *= 0.7;
            issues.push(format!("system {}: staff widths differ", k + 1));
        }
    }
    if !staves.is_empty() {
        let mut sp: Vec<f32> = staves.iter().filter(|s| s.lines == 5).map(|s| s.space).collect();
        sp.sort_by(f32::total_cmp);
        let med = sp.get(sp.len() / 2).copied().unwrap_or(0.0);
        let odd = staves.iter().filter(|s| s.lines == 5 && (s.space - med).abs() > 0.3 * med).count();
        if odd > 0 {
            conf *= 0.9;
            issues.push(format!("{odd} staff/staves with unusual line distance"));
        }
        if regions.iter().filter(|r| r.kind == Kind::System).any(|r| r.y1 - r.y0 > 0.9) {
            issues.push("one system fills the page".into());
        }
    }
    if skew.abs() > 1.5 {
        conf *= 0.9;
        issues.push(format!("skew {skew:.2} deg"));
    }

    PageResult {
        width_px: w,
        height_px: h,
        threshold: thr,
        skew_deg: skew,
        line_thickness_px: t as f32,
        line_distance_px: d,
        systems: systems.len(),
        staves,
        regions,
        confidence: conf,
        fallback: conf < p.confidence_threshold,
        issues,
    }
}

/// Deskew helper for drawing: original y of a deskewed point.
pub fn unskew_y(res: &PageResult, x: f32, y: f32) -> f32 {
    let tan = if res.skew_deg.abs() < 0.02 { 0.0 } else { res.skew_deg.to_radians().tan() };
    y + (x - res.width_px as f32 * 0.5) * tan
}
