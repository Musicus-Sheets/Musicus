//! System layout of sheet files and the system-wise reader ("system view").
//!
//! The server detects the music systems on every page of an uploaded PDF
//! once (see `musicus-server/src/layout`) and serves them as [`FileLayout`].
//! A page may be shown whole (`PageLayout::full_page`) when the detection is unsure.
//! Librarians may correct a page ([`PageCorrection`]); the served layout then
//! contains the corrected page.
//! The client turns the layout into a reading sequence ([`reading_items`])
//! and plans the views for its screen ([`plan_views`]): as many whole systems as fit at the user's minimum staff
//! height, cropped to the content width, continuing across page boundaries.
//!
//! All coordinates are page-relative (0..1 of the page width / height as
//! rendered, i.e. with the page rotation applied), like annotation
//! coordinates, so the same numbers work at any zoom.

use crate::error::ErrorCode;
use serde::{Deserialize, Serialize};

/// What a horizontal band of a page is.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RegionKind {
    /// Title, composer, running head above the first system.
    Header,
    /// One music system (one or more staves read together).
    System,
    /// Copyright line, page number, verses below the last system.
    Footer,
    /// Never shown in the system view (set by a librarian).
    Hidden,
}

/// One band of a page. Bands of a page do not overlap: `y1` of one band is
/// `y0` of the next (the cut line).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Region {
    pub kind: RegionKind,
    /// Cut lines above and below (0..1 of the page height).
    pub y0: f32,
    pub y1: f32,
    /// Rows that contain ink, inside the cut lines.
    pub ink_y0: f32,
    pub ink_y1: f32,
    /// Ink extent to the left and right (0..1 of the page width).
    pub x0: f32,
    pub x1: f32,
    /// Number of staves (0 for text regions).
    pub staves: u32,
    /// Smallest line distance of the region's staves, relative to the page
    /// height (0 for text regions). A staff is 4 line distances high.
    pub staff_space: f32,
}

/// The layout of one page.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PageLayout {
    /// Page size as rendered, in millimetres.
    pub width_mm: f32,
    pub height_mm: f32,
    /// 0..1; below [`CONFIDENCE_MIN`] the page is shown whole.
    pub confidence: f32,
    /// Show this page as a whole page in the system view (low confidence,
    /// no systems found, or a librarian's choice).
    pub full_page: bool,
    pub regions: Vec<Region>,
    /// The page was corrected by a librarian.
    #[serde(default)]
    pub corrected: bool,
}

/// Pages whose detection confidence is below this are shown whole.
pub const CONFIDENCE_MIN: f32 = 0.6;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LayoutState {
    /// Not analysed yet (the server works through new files in the background).
    Pending,
    Done,
    /// The file could not be analysed; the reader shows whole pages.
    Failed,
}

/// `GET /api/files/{sha256}/layout`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FileLayout {
    pub sha256: String,
    pub state: LayoutState,
    /// Version of the detection algorithm that produced the pages.
    pub algorithm: i64,
    pub pages: Vec<PageLayout>,
    /// Pages whose stored detection or correction this release cannot read
    /// (written by a newer release, e.g. a band kind it does not know;
    /// R-OPS-041). The server serves them as whole pages; the editor allows
    /// only resetting their correction (a-stored-fallbacks).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unreadable_pages: Vec<u32>,
}

/// A librarian's correction of one page (`PUT /api/files/{sha256}/layout/pages/{page}`).
/// The page is cut at `bounds` (increasing, 0..1 of the page height); band
/// `i` lies between `bounds[i]` and `bounds[i + 1]` and has kind `kinds[i]`.
/// Everything above the first and below the last bound is not shown.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct PageCorrection {
    pub full_page: bool,
    pub bounds: Vec<f32>,
    pub kinds: Vec<RegionKind>,
}

/// Upper limit of bands per corrected page.
pub const MAX_CORRECTED_BANDS: usize = 60;

impl PageCorrection {
    /// Checks the invariants; the error is the code of the 400 response.
    pub fn validate(&self) -> Result<(), ErrorCode> {
        if self.kinds.len() > MAX_CORRECTED_BANDS {
            return Err(ErrorCode::CorrectionTooManyBands { max: MAX_CORRECTED_BANDS as u32 });
        }
        if self.full_page && self.kinds.is_empty() {
            return Ok(());
        }
        if self.bounds.len() != self.kinds.len() + 1 || self.kinds.is_empty() {
            return Err(ErrorCode::CorrectionBoundsCount);
        }
        if self.bounds.iter().any(|b| !b.is_finite() || !(0.0..=1.0).contains(b)) {
            return Err(ErrorCode::CorrectionBoundsRange);
        }
        if self.bounds.windows(2).any(|w| w[1] - w[0] < MIN_BAND_HEIGHT) {
            return Err(ErrorCode::CorrectionBoundsOrder);
        }
        Ok(())
    }
}

/// Smallest band of a correction (0..1 of the page height).
pub const MIN_BAND_HEIGHT: f32 = 0.005;

// ---------------------------------------------------------------------------
// Reader settings (stored per user on the server, R-SEC-001)
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ReaderMode {
    /// One page at a time, fitted to the screen.
    #[default]
    Page,
    /// Whole systems at a minimum staff height, across page boundaries.
    Systems,
}

/// Minimum staff height in the system view.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StaffSize {
    S,
    #[default]
    M,
    L,
    XL,
}

impl StaffSize {
    pub const ALL: [StaffSize; 4] = [StaffSize::S, StaffSize::M, StaffSize::L, StaffSize::XL];

    /// Minimum height of a five-line staff on the screen, in millimetres.
    pub fn min_staff_mm(self) -> f64 {
        match self {
            StaffSize::S => 5.0,
            StaffSize::M => 6.0,
            StaffSize::L => 7.0,
            StaffSize::XL => 8.0,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            StaffSize::S => "S",
            StaffSize::M => "M",
            StaffSize::L => "L",
            StaffSize::XL => "XL",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        StaffSize::ALL.into_iter().find(|z| z.as_str() == s)
    }
}

impl ReaderMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ReaderMode::Page => "page",
            ReaderMode::Systems => "systems",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "page" => Some(ReaderMode::Page),
            "systems" => Some(ReaderMode::Systems),
            _ => None,
        }
    }
}

/// `PUT /api/auth/reader-settings`; also part of `CurrentUser`.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ReaderSettings {
    pub mode: ReaderMode,
    pub staff_size: StaffSize,
}

// ---------------------------------------------------------------------------
// Reading sequence and views
// ---------------------------------------------------------------------------

/// Headers and footers lower than this (0..1 of the page height) are page
/// furniture (running head, page number, copyright) and are skipped, except
/// the header of the first page (title of the piece).
pub const TALL_TEXT: f32 = 0.06;
/// Padding above and below the ink of a system, in line distances.
pub const SYSTEM_PADDING_SPACES: f32 = 1.0;
/// Overlap rule: a view with at least this many systems is followed by a view
/// that starts with its last system; smaller views are followed by the next
/// system (no overlap).
pub const OVERLAP_MIN_SYSTEMS: usize = 3;
/// Physical size of a CSS pixel assumed for the staff height setting: 132
/// CSS px per inch (iPad; most 8–11" Android tablets have 120–160). Browsers
/// do not tell the real screen density.
pub const CSS_PX_PER_MM: f64 = 132.0 / 25.4;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    System,
    /// A header or footer that is shown (title, verses).
    Text,
    /// A whole page (low confidence or no systems).
    Page,
}

/// One entry of the reading sequence: a crop of one page.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Item {
    /// 0-based page index.
    pub page: u32,
    pub kind: ItemKind,
    /// Crop, 0..1 of the page.
    pub x0: f64,
    pub x1: f64,
    pub y0: f64,
    pub y1: f64,
    /// Page size in millimetres.
    pub page_w_mm: f64,
    pub page_h_mm: f64,
    /// Height of the smallest staff on paper, in millimetres (0: no staff).
    pub staff_mm: f64,
}

/// The reading sequence of a file: every page in order; per page either its
/// shown regions or the whole page.
pub fn reading_items(pages: &[PageLayout]) -> Vec<Item> {
    let mut out = Vec::new();
    for (index, p) in pages.iter().enumerate() {
        let (w, h) = (p.width_mm as f64, p.height_mm as f64);
        let page = index as u32;
        let has_systems = p.regions.iter().any(|r| r.kind == RegionKind::System);
        if p.full_page || !has_systems {
            out.push(Item {
                page,
                kind: ItemKind::Page,
                x0: 0.0,
                x1: 1.0,
                y0: 0.0,
                y1: 1.0,
                page_w_mm: w,
                page_h_mm: h,
                staff_mm: 0.0,
            });
            continue;
        }
        for r in &p.regions {
            let kind = match r.kind {
                RegionKind::System => ItemKind::System,
                RegionKind::Header | RegionKind::Footer
                    if (r.kind == RegionKind::Header && index == 0) || r.ink_y1 - r.ink_y0 > TALL_TEXT =>
                {
                    ItemKind::Text
                }
                _ => continue,
            };
            let pad = if kind == ItemKind::System { r.staff_space * SYSTEM_PADDING_SPACES } else { 0.005 };
            let y0 = (r.ink_y0 - pad).max(r.y0).clamp(0.0, 1.0) as f64;
            let y1 = (r.ink_y1 + pad).min(r.y1).clamp(0.0, 1.0) as f64;
            if y1 <= y0 || r.x1 <= r.x0 {
                continue;
            }
            out.push(Item {
                page,
                kind,
                x0: r.x0.clamp(0.0, 1.0) as f64,
                x1: r.x1.clamp(0.0, 1.0) as f64,
                y0,
                y1,
                page_w_mm: w,
                page_h_mm: h,
                staff_mm: if kind == ItemKind::System { 4.0 * r.staff_space as f64 * h } else { 0.0 },
            });
        }
    }
    out
}

/// The available screen area.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    /// CSS pixels.
    pub width: f64,
    pub height: f64,
    /// CSS pixels per physical millimetre, normally [`CSS_PX_PER_MM`].
    pub px_per_mm: f64,
}

/// A contiguous part of one page shown in a view (0..1 of the page).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub page: u32,
    pub x0: f64,
    pub x1: f64,
    pub y0: f64,
    pub y1: f64,
}

/// One screenful: items `first..=last`. On each page the view shows ONE
/// contiguous crop of the original page (`segments`, one per page touched,
/// stacked directly below each other): from the top of its first item to the
/// bottom of its last, with everything in between (gaps, text, hidden bands).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct View {
    pub first: usize,
    pub last: usize,
    /// CSS pixels per millimetre of paper.
    pub scale: f64,
    pub segments: Vec<Segment>,
    /// The minimum staff height is not reached: the music is wider than the
    /// screen at that size (shown at full width), or one item is taller than
    /// the screen.
    pub below_min: bool,
}

/// Plans all views of a reading sequence (the algorithm of the technical
/// documentation, "System view"):
///
/// 1. One horizontal crop for the whole document: the union of the
///    horizontal extents of all system and text items (mm); `fit_w` = screen
///    width / its width. One scale for the whole document: `s` =
///    min(`needed`, `fit_w`), `needed` = minimum staff mm × px per mm /
///    smallest staff on paper of the document. So views never change scale or
///    move sideways, and music wider than the screen at the minimum height is
///    shown at full width (`below_min`).
/// 2. A view starts at item `start`. Whole pages are always alone (fitted).
/// 3. The view's height is the sum over the pages it touches of
///    (bottom of the last item − top of the first item on that page): the
///    contiguous crops, gaps included. The view grows while the next item is
///    not a whole page and the height × `s` fits the screen height.
/// 4. A view never ends with a header; it moves to the next view.
/// 5. A single item taller than the screen is scaled down to fit (`below_min`).
/// 6. Next start: the view's last system if the view holds at least
///    [`OVERLAP_MIN_SYSTEMS`] systems (one-system overlap), else the item
///    after the view. A view that would consist of only the overlapped
///    system is skipped.
pub fn plan_views(items: &[Item], screen: Screen, min_staff_mm: f64) -> Vec<View> {
    let mut views = Vec::new();
    if screen.width < 1.0 || screen.height < 1.0 || items.is_empty() {
        return views;
    }
    let music = || items.iter().filter(|i| i.kind != ItemKind::Page);
    let dx0 = music().map(|i| i.x0 * i.page_w_mm).fold(f64::MAX, f64::min);
    let dx1 = music().map(|i| i.x1 * i.page_w_mm).fold(0.0, f64::max);
    let fit_w = if dx1 > dx0 { screen.width / (dx1 - dx0).max(1.0) } else { 0.0 };
    let staff = music().map(|i| i.staff_mm).filter(|&s| s > 0.0).fold(f64::MAX, f64::min);
    let needed = if staff == f64::MAX { 0.0 } else { min_staff_mm * screen.px_per_mm / staff };
    let s = if needed > 0.0 { needed.min(fit_w) } else { fit_w };

    let segments = |a: usize, b: usize| -> Vec<Segment> {
        let mut out: Vec<Segment> = Vec::new();
        for i in &items[a..=b] {
            let (x0, x1) = if i.kind == ItemKind::Page {
                (0.0, 1.0)
            } else {
                ((dx0 / i.page_w_mm).clamp(0.0, 1.0), (dx1 / i.page_w_mm).clamp(0.0, 1.0))
            };
            match out.last_mut() {
                Some(seg) if seg.page == i.page => {
                    seg.y0 = seg.y0.min(i.y0);
                    seg.y1 = seg.y1.max(i.y1);
                }
                _ => out.push(Segment { page: i.page, x0, x1, y0: i.y0, y1: i.y1 }),
            }
        }
        out
    };
    let height_mm = |a: usize, b: usize| -> f64 {
        segments(a, b)
            .iter()
            .map(|g| (g.y1 - g.y0) * items[a..=b].iter().find(|i| i.page == g.page).map_or(0.0, |i| i.page_h_mm))
            .sum()
    };
    let systems = |a: usize, b: usize| items[a..=b].iter().filter(|i| i.kind == ItemKind::System).count();

    let mut start = 0;
    let mut overlapped = false;
    while start < items.len() {
        let mut last = start;
        if items[start].kind != ItemKind::Page {
            while last + 1 < items.len()
                && items[last + 1].kind != ItemKind::Page
                && height_mm(start, last + 1) * s <= screen.height
            {
                last += 1;
            }
            while last > start && items[last].kind == ItemKind::Text && is_header_like(items, last) {
                last -= 1;
            }
        }
        if overlapped && last == start && start + 1 < items.len() {
            overlapped = false;
            start += 1;
            continue;
        }
        let it = &items[start];
        let scale = if it.kind == ItemKind::Page {
            (screen.width / it.page_w_mm).min(screen.height / it.page_h_mm)
        } else {
            s.min(screen.height / height_mm(start, last).max(1e-6))
        };
        let below_min = it.kind != ItemKind::Page && scale + 1e-9 < needed;
        views.push(View { first: start, last, scale, segments: segments(start, last), below_min });
        if last + 1 >= items.len() {
            break;
        }
        overlapped = systems(start, last) >= OVERLAP_MIN_SYSTEMS && items[last].kind == ItemKind::System;
        start = if overlapped { last } else { last + 1 };
    }
    views
}

/// A text item that belongs to the music after it (a title): the next item
/// is on the same page or the text is at the top of its page.
fn is_header_like(items: &[Item], i: usize) -> bool {
    match items.get(i + 1) {
        Some(next) => next.page == items[i].page || items[i].y0 < 0.5,
        None => false,
    }
}

/// The view to show for reading position `item`: the last view that starts
/// at or before it. Views cover the sequence without gaps, so that view
/// contains the item; with overlap, an item that ends one view and starts the
/// next gives the next one (the reading position is the first item of a view).
pub fn view_for_item(views: &[View], item: usize) -> usize {
    views.iter().rposition(|v| v.first <= item).unwrap_or(0)
}

/// The first item of `page` (or of the next page that has items).
pub fn first_item_of_page(items: &[Item], page: u32) -> usize {
    items.iter().position(|i| i.page >= page).unwrap_or(items.len().saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn system(page: u32, y0: f64, y1: f64, staff_mm: f64) -> Item {
        Item { page, kind: ItemKind::System, x0: 0.1, x1: 0.9, y0, y1, page_w_mm: 210.0, page_h_mm: 297.0, staff_mm }
    }

    /// An A4 part with `n` systems per page, each 1/12 of the page high,
    /// content 168 mm wide.
    fn part(pages: u32, n: u32, staff_mm: f64) -> Vec<Item> {
        let mut v = Vec::new();
        for p in 0..pages {
            for k in 0..n {
                let y0 = 0.05 + k as f64 / 12.0;
                v.push(system(p, y0, y0 + 1.0 / 12.0, staff_mm));
            }
        }
        v
    }

    fn screen(w: f64, h: f64) -> Screen {
        Screen { width: w, height: h, px_per_mm: CSS_PX_PER_MM }
    }

    #[test]
    fn whole_systems_at_the_minimum_size_with_one_system_overlap() {
        // 1000 x 700 CSS px; 168 mm content ⇒ fit_w ≈ 5.95 px/mm; needed = 5 · 5.197 / 7 ≈ 3.71 px/mm.
        // Systems are contiguous (1/12 page = 24.75 mm each) ⇒ 700 / (n · 24.75 · 3.71) ⇒ n ≤ 7.6 ⇒ 7.
        let items = part(2, 10, 7.0);
        let views = plan_views(&items, screen(1000.0, 700.0), 5.0);
        let v = &views[0];
        assert_eq!((v.first, v.last), (0, 6));
        assert!(!v.below_min);
        // One scale for every view of the document.
        assert!(views.iter().all(|w| (w.scale - v.scale).abs() < 1e-9));
        assert!((v.scale - 5.0 * CSS_PX_PER_MM / 7.0).abs() < 1e-9);
        // Overlap: the next view starts with the last system of this one.
        assert_eq!(views[1].first, 6);
        // The sequence continues across the page boundary: one segment per page.
        let across = views.iter().find(|v| items[v.first].page == 0 && items[v.last].page == 1).unwrap();
        assert_eq!(across.segments.len(), 2);
        assert_eq!(views.last().unwrap().last, items.len() - 1);
        for w in views.windows(2) {
            assert!(w[1].first <= w[0].last + 1 && w[1].first > w[0].first);
        }
    }

    #[test]
    fn a_view_is_one_contiguous_crop_per_page_with_one_x_range() {
        // Systems with gaps between them (and different widths): the crop runs from
        // the first system's top to the last one's bottom, gaps included.
        let mut items = vec![
            system(0, 0.10, 0.18, 7.0),
            system(0, 0.30, 0.38, 7.0),
            system(0, 0.50, 0.58, 7.0),
            system(1, 0.10, 0.18, 7.0),
        ];
        items[1].x0 = 0.2;
        items[3].x1 = 0.95;
        let views = plan_views(&items, screen(1000.0, 3000.0), 5.0);
        assert_eq!(views.len(), 1);
        let segs = &views[0].segments;
        assert_eq!(segs.len(), 2);
        assert_eq!((segs[0].page, segs[0].y0, segs[0].y1), (0, 0.10, 0.58));
        // One horizontal crop for all: union of all items (0.1 .. 0.95 of 210 mm).
        for g in segs {
            assert!((g.x0 - 0.1).abs() < 1e-9 && (g.x1 - 0.95).abs() < 1e-9);
        }
        // The gaps count: 0.48 + 0.08 of 297 mm at the document scale must fit.
        let views = plan_views(&items, screen(1000.0, 400.0), 5.0);
        let h = |v: &View| v.segments.iter().map(|g| (g.y1 - g.y0) * 297.0).sum::<f64>() * v.scale;
        assert!(views.iter().all(|v| h(v) <= 400.0 + 1e-6));
        assert!(views[0].last < 2, "the gaps between systems take space too");
    }

    #[test]
    fn small_views_advance_without_overlap() {
        // Big minimum: only 2 systems fit ⇒ no overlap.
        let items = part(1, 10, 7.0);
        let views = plan_views(&items, screen(1000.0, 330.0), 8.0);
        assert_eq!((views[0].first, views[0].last), (0, 1));
        assert_eq!(views[1].first, 2);
        // One system per view ⇒ advance by one.
        let views = plan_views(&items, screen(1000.0, 200.0), 8.0);
        assert!(views.iter().all(|v| v.first == v.last));
        assert_eq!(views.len(), 10);
    }

    #[test]
    fn too_wide_music_is_shown_at_full_width() {
        // 400 px wide: fit_w = 400/168 = 2.38 < needed (5.197*6/7 = 4.45).
        let items = part(1, 10, 7.0);
        let views = plan_views(&items, screen(400.0, 600.0), 6.0);
        assert!(views[0].below_min);
        assert!((views[0].scale - 400.0 / 168.0).abs() < 1e-6);
        // 600 / (n * 24.75 * 2.38) ⇒ 10 systems fit at full width.
        assert_eq!(views[0].last, 9);
    }

    #[test]
    fn whole_pages_and_titles() {
        let mut items = part(2, 3, 7.0);
        items.insert(3, Item { kind: ItemKind::Page, x0: 0.0, x1: 1.0, y0: 0.0, y1: 1.0, ..system(1, 0.0, 1.0, 0.0) });
        for i in items.iter_mut().skip(4) {
            i.page = 2;
        }
        let views = plan_views(&items, screen(1000.0, 2000.0), 5.0);
        let page_view = views.iter().find(|v| v.first == 3).unwrap();
        assert_eq!(page_view.last, 3, "a whole page is alone");
        assert_eq!(page_view.segments[0], Segment { page: 1, x0: 0.0, x1: 1.0, y0: 0.0, y1: 1.0 });
        // A title is never the last item of a view.
        let mut items = part(1, 4, 7.0);
        items.insert(2, Item { kind: ItemKind::Text, staff_mm: 0.0, ..system(0, 0.4, 0.45, 0.0) });
        let views = plan_views(&items, screen(1000.0, 300.0), 5.0);
        assert!(views.iter().all(|v| items[v.last].kind != ItemKind::Text));
    }

    #[test]
    fn position_survives_a_new_plan() {
        let items = part(3, 10, 7.0);
        let landscape = plan_views(&items, screen(1000.0, 700.0), 5.0);
        let portrait = plan_views(&items, screen(700.0, 1000.0), 5.0);
        let item = landscape[2].first;
        let v = &portrait[view_for_item(&portrait, item)];
        assert!(v.first <= item && item <= v.last);
        // With overlap, the first item of a view selects that view, not the one before.
        for (i, v) in landscape.iter().enumerate() {
            assert_eq!(view_for_item(&landscape, v.first), i);
        }
        assert_eq!(first_item_of_page(&items, 1), 10);
    }

    #[test]
    fn reading_items_skip_page_furniture() {
        let region = |kind, y0: f32, y1: f32| Region {
            kind,
            y0,
            y1,
            ink_y0: y0,
            ink_y1: y1,
            x0: 0.1,
            x1: 0.9,
            staves: (kind == RegionKind::System) as u32,
            staff_space: if kind == RegionKind::System { 0.006 } else { 0.0 },
        };
        let page = |regions| PageLayout {
            width_mm: 210.0,
            height_mm: 297.0,
            confidence: 1.0,
            full_page: false,
            regions,
            corrected: false,
        };
        let pages = vec![
            page(vec![
                region(RegionKind::Header, 0.0, 0.1),
                region(RegionKind::System, 0.1, 0.5),
                region(RegionKind::Footer, 0.95, 0.97),
            ]),
            page(vec![
                region(RegionKind::Header, 0.0, 0.03),
                region(RegionKind::System, 0.05, 0.5),
                region(RegionKind::Hidden, 0.5, 0.6),
                region(RegionKind::Footer, 0.6, 0.9),
            ]),
            PageLayout { full_page: true, ..page(vec![region(RegionKind::System, 0.1, 0.5)]) },
        ];
        let items = reading_items(&pages);
        let kinds: Vec<(u32, ItemKind)> = items.iter().map(|i| (i.page, i.kind)).collect();
        assert_eq!(
            kinds,
            vec![
                (0, ItemKind::Text),
                (0, ItemKind::System),
                (1, ItemKind::System),
                (1, ItemKind::Text),
                (2, ItemKind::Page)
            ]
        );
        // Staff height on paper: 4 × 0.006 × 297 mm.
        assert!((items[1].staff_mm - 7.128).abs() < 1e-3);
    }

    #[test]
    fn corrections_are_validated() {
        let ok = PageCorrection {
            full_page: false,
            bounds: vec![0.1, 0.4, 0.8],
            kinds: vec![RegionKind::System, RegionKind::Hidden],
        };
        assert!(ok.validate().is_ok());
        assert!(PageCorrection { full_page: true, ..Default::default() }.validate().is_ok());
        assert!(
            PageCorrection { bounds: vec![0.4, 0.1], kinds: vec![RegionKind::System], ..ok.clone() }
                .validate()
                .is_err()
        );
        assert!(
            PageCorrection { bounds: vec![0.1, 1.4], kinds: vec![RegionKind::System], ..ok.clone() }
                .validate()
                .is_err()
        );
        assert!(PageCorrection { bounds: vec![0.1], ..ok }.validate().is_err());
    }

    #[test]
    fn unknown_region_kinds_do_not_parse() {
        // A band kind of a newer release is a parse error (the server then
        // serves the page whole, `effective_layout`); unknown fields are not.
        let page = |kind: &str| {
            format!(
                r#"{{"width_mm":210,"height_mm":297,"confidence":1,"full_page":false,"later_field":1,"regions":[
                {{"kind":"{kind}","y0":0.1,"y1":0.5,"ink_y0":0.1,"ink_y1":0.5,"x0":0.1,"x1":0.9,"staves":1,"staff_space":0.006}}]}}"#
            )
        };
        assert!(serde_json::from_str::<PageLayout>(&page("system")).is_ok());
        assert!(serde_json::from_str::<PageLayout>(&page("ossia")).is_err());
        assert!(
            serde_json::from_str::<PageCorrection>(r#"{"full_page":false,"bounds":[0,1],"kinds":["ossia"]}"#).is_err()
        );
        let old: FileLayout =
            serde_json::from_str(r#"{"sha256":"x","state":"done","algorithm":1,"pages":[]}"#).unwrap();
        assert!(old.unreadable_pages.is_empty());
    }

    #[test]
    fn settings_round_trip() {
        for z in StaffSize::ALL {
            assert_eq!(StaffSize::parse(z.as_str()), Some(z));
        }
        assert!(StaffSize::S.min_staff_mm() < StaffSize::XL.min_staff_mm());
        assert_eq!(ReaderMode::parse("systems"), Some(ReaderMode::Systems));
        let s: ReaderSettings = serde_json::from_str(r#"{"mode":"systems","staff_size":"XL"}"#).unwrap();
        assert_eq!(s, ReaderSettings { mode: ReaderMode::Systems, staff_size: StaffSize::XL });
    }
}
