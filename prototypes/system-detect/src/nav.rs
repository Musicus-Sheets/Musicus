//! Simulation of the system-wise reader ("concept A"): which whole systems are shown per view,
//! at which scale, with a one-system overlap, across page boundaries.

use crate::detect::{Kind, PageResult};
use serde::Serialize;

/// One thing the reader can show: a system, a text block (title, verses), or a whole page
/// (fallback for pages without reliable detection).
#[derive(Serialize, Clone, Debug)]
pub struct Item {
    /// 0-based index into the simulated pages.
    pub page: usize,
    pub kind: &'static str,
    /// Page-relative crop (0..1).
    pub x0: f32,
    pub x1: f32,
    pub y0: f32,
    pub y1: f32,
    /// Staff height (4 line distances) in mm on paper; 0 for text and fallback pages.
    pub staff_mm: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct PageGeom {
    pub w_mm: f32,
    pub h_mm: f32,
}

#[derive(Serialize, Clone, Debug)]
pub struct Screen {
    pub name: String,
    pub w_px: u32,
    pub h_px: u32,
    pub ppi: f32,
}

impl Screen {
    pub fn w_mm(&self) -> f32 {
        self.w_px as f32 / self.ppi * 25.4
    }
    pub fn h_mm(&self) -> f32 {
        self.h_px as f32 / self.ppi * 25.4
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct View {
    /// Item indices shown (consecutive).
    pub first: usize,
    pub last: usize,
    /// Screen mm per paper mm.
    pub scale: f32,
    /// Smallest staff height on screen in mm (0 if the view has no detected staff).
    pub staff_mm_screen: f32,
    /// Even a single item does not fit at the minimum staff height.
    pub below_min: bool,
    /// Pages touched (0-based).
    pub pages: Vec<usize>,
}

/// Turns detection results into the reading sequence. Headers are kept on the first page and
/// when large (movement titles, verses); small headers/footers (running heads, copyright) are
/// dropped. Pages that fell back become one full-page item.
pub fn items(pages: &[(PageGeom, &PageResult)]) -> Vec<Item> {
    let mut out = Vec::new();
    for (pi, (g, res)) in pages.iter().enumerate() {
        let has_systems = res.regions.iter().any(|r| r.kind == Kind::System);
        if res.fallback || !has_systems {
            out.push(Item { page: pi, kind: "page", x0: 0.0, x1: 1.0, y0: 0.0, y1: 1.0, staff_mm: 0.0 });
            continue;
        }
        for r in &res.regions {
            let pad = if r.kind == Kind::System { r.staff_space } else { 0.005 };
            let (y0, y1) = ((r.ink_y0 - pad).max(r.y0), (r.ink_y1 + pad).min(r.y1));
            let tall = r.ink_y1 - r.ink_y0 > 0.06;
            let keep = match r.kind {
                Kind::System => true,
                Kind::Header => pi == 0 || tall,
                Kind::Footer => tall,
            };
            if !keep {
                continue;
            }
            out.push(Item {
                page: pi,
                kind: match r.kind {
                    Kind::System => "system",
                    Kind::Header => "header",
                    Kind::Footer => "footer",
                },
                x0: r.x0,
                x1: r.x1,
                y0,
                y1,
                staff_mm: 4.0 * r.staff_space * g.h_mm,
            });
        }
    }
    out
}

/// Plans the views for `screen` and a minimum on-screen staff height `min_staff_mm`.
pub fn plan(items: &[Item], pages: &[PageGeom], screen: &Screen, min_staff_mm: f32) -> Vec<View> {
    let (sw, sh) = (screen.w_mm(), screen.h_mm());
    // Scales at which items[a..=b] fit the screen width (content-width crop) and height (stacked).
    let fit_w = |a: usize, b: usize| -> f32 {
        let x0 = items[a..=b].iter().map(|it| it.x0 * pages[it.page].w_mm).fold(f32::MAX, f32::min);
        let x1 = items[a..=b].iter().map(|it| it.x1 * pages[it.page].w_mm).fold(0.0, f32::max);
        sw / (x1 - x0)
    };
    let fit_h = |a: usize, b: usize| -> f32 {
        let hsum: f32 = items[a..=b].iter().map(|it| (it.y1 - it.y0) * pages[it.page].h_mm).sum();
        sh / hsum
    };
    let fit = |a: usize, b: usize| fit_w(a, b).min(fit_h(a, b));
    let min_staff = |a: usize, b: usize| -> f32 {
        items[a..=b].iter().map(|it| it.staff_mm).filter(|&s| s > 0.0).fold(f32::MAX, f32::min)
    };
    // Scale needed so that the smallest staff reaches the minimum height.
    let needed = |a: usize, b: usize| -> f32 {
        let s = min_staff(a, b);
        if s == f32::MAX { 0.0 } else { min_staff_mm / s }
    };
    let is_page = |i: usize| items[i].kind == "page";
    let mut views = Vec::new();
    let mut start = 0;
    let mut overlapped = false;
    while start < items.len() {
        let mut last = start;
        // A full-page fallback item is always alone.
        if !is_page(start) {
            // Target scale: the minimum staff height, or the width-limited maximum if the
            // content is too wide for that (no horizontal reflow).
            let target = |b: usize| needed(start, b).min(fit_w(start, b));
            while last + 1 < items.len() && !is_page(last + 1) && fit(start, last + 1) >= target(last + 1) {
                last += 1;
            }
        }
        // A title belongs to the music after it: never end a view on a header.
        while last > start && items[last].kind == "header" {
            last -= 1;
        }
        // Only the system already shown at the bottom of the previous view fits (because a
        // title follows it): skip it instead of spending a page turn on it.
        if overlapped && last == start {
            overlapped = false;
            start += 1;
            continue;
        }
        let scale = fit(start, last);
        let ms = min_staff(start, last);
        let mut pages_touched: Vec<usize> = items[start..=last].iter().map(|it| it.page).collect();
        pages_touched.dedup();
        views.push(View {
            first: start,
            last,
            scale,
            staff_mm_screen: if ms == f32::MAX { 0.0 } else { ms * scale },
            below_min: scale < needed(start, last),
            pages: pages_touched,
        });
        if last + 1 >= items.len() {
            break;
        }
        // One-system overlap: the last system becomes the first of the next view.
        overlapped = last > start && items[last].kind == "system";
        start = if overlapped { last } else { last + 1 };
    }
    views
}

/// Staff height on screen with today's full-page fit.
pub fn page_fit_scale(page: PageGeom, screen: &Screen) -> f32 {
    (screen.w_mm() / page.w_mm).min(screen.h_mm() / page.h_mm)
}
