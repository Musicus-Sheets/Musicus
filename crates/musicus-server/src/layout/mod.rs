//! System layout of uploaded PDFs: where the music systems are on every page.
//!
//! Runs once per file (blobs are immutable, R-FILE-004) in a background
//! worker ([`worker`]), never in a request. Each page is rendered in
//! greyscale with hayro and analysed by [`detect`]; the result is stored as
//! `musicus_api::layout::PageLayout` JSON in `file_layouts`, together with
//! [`ALGORITHM_VERSION`]. Raising that version makes the worker analyse
//! every file again (librarians' corrections are kept separately and win).
//! Exact algorithm: `docs/technical/chapters/12-system-view.typ`.

pub mod detect;
pub mod worker;

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings, render};
use musicus_api::layout::{CONFIDENCE_MIN, PageLayout, Region, RegionKind};

/// Version of the detection algorithm. Raise it whenever `detect` changes
/// its results; the worker then re-analyses all files in the background.
pub const ALGORITHM_VERSION: i64 = 1;
/// Pages are rendered at this resolution ...
pub const RENDER_DPI: f32 = 150.0;
/// ... but never with a longer side than an A4 page at `RENDER_DPI` (1754 px):
/// facsimiles declaring huge page sizes would otherwise cost seconds and
/// hundreds of MB; detection works in multiples of the line distance anyway.
pub const MAX_LONG_SIDE_PX: f32 = RENDER_DPI / 72.0 * 842.0;

/// 8-bit greyscale image, row-major, 0 = black.
pub struct Gray {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

const MM_PER_POINT: f32 = 25.4 / 72.0;

/// Analyses every page of a PDF. Errors if hayro cannot read the file.
pub fn analyse_pdf(data: Vec<u8>) -> Result<Vec<PageLayout>, String> {
    let pdf = Pdf::new(data).map_err(|e| format!("{e:?}"))?;
    let mut out = Vec::new();
    for page in pdf.pages().iter() {
        let (w_pt, h_pt) = page.render_dimensions();
        let scale = (RENDER_DPI / 72.0).min(MAX_LONG_SIDE_PX / w_pt.max(h_pt).max(1.0));
        let pixmap = render(
            page,
            &RenderCache::new(),
            &InterpreterSettings::default(),
            &RenderSettings::default(),
            &PixmapSettings { x_scale: scale, y_scale: scale, bg_color: WHITE },
        );
        let rgba = pixmap.data_as_u8_slice();
        let gray = Gray {
            w: pixmap.width() as usize,
            h: pixmap.height() as usize,
            px: rgba
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| ((p[0] as u32 * 77 + p[1] as u32 * 150 + p[2] as u32 * 29) >> 8) as u8)
                .collect(),
        };
        out.push(analyse_page(&gray, w_pt * MM_PER_POINT, h_pt * MM_PER_POINT));
    }
    Ok(out)
}

/// Detection result of one rendered page as the API type.
pub fn analyse_page(gray: &Gray, width_mm: f32, height_mm: f32) -> PageLayout {
    if gray.w < 16 || gray.h < 16 {
        return PageLayout { width_mm, height_mm, confidence: 0.0, full_page: true, regions: vec![], corrected: false };
    }
    let r = detect::detect(gray);
    let regions: Vec<Region> = r
        .regions
        .iter()
        .map(|g| Region {
            kind: match g.kind {
                detect::Kind::Header => RegionKind::Header,
                detect::Kind::System => RegionKind::System,
                detect::Kind::Footer => RegionKind::Footer,
            },
            y0: g.y0,
            y1: g.y1,
            ink_y0: g.ink_y0,
            ink_y1: g.ink_y1,
            x0: g.x0,
            x1: g.x1,
            staves: g.staves as u32,
            staff_space: g.staff_space,
        })
        .collect();
    let has_systems = regions.iter().any(|g| g.kind == RegionKind::System);
    PageLayout {
        width_mm,
        height_mm,
        confidence: r.confidence,
        full_page: r.confidence < CONFIDENCE_MIN || !has_systems,
        regions,
        corrected: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic engraved page (A4 at 100 dpi = 827 × 1169 px): a title, then
    /// `systems` systems of `staves` staves (line distance 9 px, thickness 2 px),
    /// joined by a barline at the left edge when `staves > 1`, with note
    /// heads, stems and a dynamic mark under each system.
    pub fn page(systems: usize, staves: usize) -> Gray {
        let (w, h) = (827usize, 1169usize);
        let mut px = vec![255u8; w * h];
        let mut ink = |x0: usize, y0: usize, x1: usize, y1: usize| {
            for y in y0..y1.min(h) {
                for x in x0..x1.min(w) {
                    px[y * w + x] = 0;
                }
            }
        };
        // Title: blocky "letters".
        for k in 0..12 {
            ink(250 + k * 28, 60, 250 + k * 28 + 18, 90);
        }
        let (left, right, d) = (70usize, 760usize, 9usize);
        let staff_h = 4 * d;
        let staff_gap = 6 * d;
        let system_h = staves * staff_h + (staves - 1) * staff_gap;
        let mut y = 170;
        for s in 0..systems {
            let top = y;
            for k in 0..staves {
                let st = top + k * (staff_h + staff_gap);
                for l in 0..5 {
                    ink(left, st + l * d, right, st + l * d + 2);
                }
                // Notes: heads and stems, some above the staff with a ledger line.
                for n in 0..24 {
                    let x = left + 40 + n * 27;
                    let hy = st + ((n * 7 + s * 3 + k) % 9) * d / 2;
                    ink(x, hy, x + 10, hy + 7);
                    ink(x + 9, hy - 3 * d, x + 11, hy + 3);
                    if n % 7 == 0 {
                        ink(x - 4, st - d, x + 14, st - d + 2);
                        ink(x, st - d - 3, x + 10, st - d + 4);
                    }
                }
                // Barlines through the staff.
                for b in 1..4 {
                    let x = left + b * (right - left) / 4;
                    ink(x, st, x + 2, st + staff_h + 2);
                }
            }
            if staves > 1 {
                ink(left, top, left + 3, top + system_h + 2);
            }
            // A dynamic mark ("mf") below the system.
            ink(left + 30, top + system_h + 2 * d, left + 50, top + system_h + 3 * d);
            y = top + system_h + 9 * d;
            let _ = s;
        }
        // Page number at the bottom.
        ink(400, 1120, 420, 1135);
        Gray { w, h, px }
    }

    #[test]
    fn finds_single_staff_systems_of_a_part() {
        let p = analyse_page(&page(8, 1), 210.0, 297.0);
        let systems: Vec<&Region> = p.regions.iter().filter(|r| r.kind == RegionKind::System).collect();
        assert_eq!(systems.len(), 8, "{:?}", p.regions);
        assert!(systems.iter().all(|r| r.staves == 1));
        assert!(!p.full_page && p.confidence >= CONFIDENCE_MIN);
        // Title above the first system is the header; the page number the footer.
        assert_eq!(p.regions.first().map(|r| r.kind), Some(RegionKind::Header));
        assert_eq!(p.regions.last().map(|r| r.kind), Some(RegionKind::Footer));
        // Line distance 9 px of 1169 px; bands are contiguous and the dynamic mark below
        // a staff stays in its system.
        assert!((systems[0].staff_space - 9.0 / 1169.0).abs() < 0.0015);
        for w in p.regions.windows(2) {
            assert!((w[0].y1 - w[1].y0).abs() < 1e-6);
        }
        let mark = (170.0 + 36.0 + 18.0) / 1169.0;
        assert!(systems[0].y0 < mark && mark < systems[0].y1);
    }

    #[test]
    fn joins_staves_connected_by_a_barline() {
        let p = analyse_page(&page(4, 2), 210.0, 297.0);
        let staves: Vec<u32> = p.regions.iter().filter(|r| r.kind == RegionKind::System).map(|r| r.staves).collect();
        assert_eq!(staves, vec![2, 2, 2, 2], "{:?}", p.regions);
    }

    #[test]
    fn text_and_blank_pages_have_no_systems() {
        let mut text = Gray { w: 827, h: 1169, px: vec![255; 827 * 1169] };
        for line in 0..40 {
            for word in 0..12 {
                for y in 100 + line * 24..100 + line * 24 + 12 {
                    for x in 60 + word * 60..60 + word * 60 + 45 {
                        if (x / 6 + y / 3) % 3 != 0 {
                            text.px[y * 827 + x] = 0;
                        }
                    }
                }
            }
        }
        let p = analyse_page(&text, 210.0, 297.0);
        assert!(p.full_page && p.regions.is_empty());
        let blank = Gray { w: 827, h: 1169, px: vec![255; 827 * 1169] };
        assert!(analyse_page(&blank, 210.0, 297.0).full_page);
    }

    #[test]
    fn ink_right_below_the_last_staff_stays_with_it() {
        // An "8vb" line 2 spaces below the last staff must not become the footer.
        let mut g = page(3, 1);
        let last_bottom = 170 + 2 * (36 + 81) + 36;
        for y in last_bottom + 20..last_bottom + 22 {
            for x in 100..400 {
                if (x / 8) % 2 == 0 {
                    g.px[y * 827 + x] = 0;
                }
            }
        }
        let p = analyse_page(&g, 210.0, 297.0);
        let last = p.regions.iter().rfind(|r| r.kind == RegionKind::System).unwrap();
        assert!(last.ink_y1 * 1169.0 >= (last_bottom + 21) as f32, "{:?}", p.regions);
    }
}
