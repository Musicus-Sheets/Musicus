//! Overlay drawing: staff lines, region boxes, cut lines, staff counts, confidence.

use crate::detect::{Kind, PageResult, unskew_y};
use crate::raster::{Gray, Rgb};

const DIGITS: [[u8; 5]; 12] = [
    [0b111, 0b101, 0b101, 0b101, 0b111], // 0
    [0b010, 0b110, 0b010, 0b010, 0b111], // 1
    [0b111, 0b001, 0b111, 0b100, 0b111], // 2
    [0b111, 0b001, 0b111, 0b001, 0b111], // 3
    [0b101, 0b101, 0b111, 0b001, 0b001], // 4
    [0b111, 0b100, 0b111, 0b001, 0b111], // 5
    [0b111, 0b100, 0b111, 0b101, 0b111], // 6
    [0b111, 0b001, 0b010, 0b010, 0b010], // 7
    [0b111, 0b101, 0b111, 0b101, 0b111], // 8
    [0b111, 0b101, 0b111, 0b001, 0b111], // 9
    [0b000, 0b000, 0b000, 0b000, 0b010], // .
    [0b000, 0b000, 0b111, 0b000, 0b000], // -
];

/// Draws digits, '.' and '-' with a 3x5 font scaled by `s`.
pub fn text(img: &mut Rgb, x: i64, y: i64, s: i64, txt: &str, c: [u8; 3]) {
    let mut cx = x;
    for ch in txt.chars() {
        let gi = match ch {
            '0'..='9' => ch as usize - '0' as usize,
            '.' => 10,
            '-' => 11,
            _ => {
                cx += 2 * s;
                continue;
            }
        };
        for (row, bits) in DIGITS[gi].iter().enumerate() {
            for col in 0..3 {
                if bits & (0b100 >> col) != 0 {
                    for yy in 0..s {
                        for xx in 0..s {
                            img.put(cx + col * s + xx, y + row as i64 * s + yy, c);
                        }
                    }
                }
            }
        }
        cx += 4 * s;
    }
}

pub fn rect_fill(img: &mut Rgb, x0: i64, y0: i64, x1: i64, y1: i64, c: [u8; 3], a: f32) {
    for y in y0..y1 {
        for x in x0..x1 {
            img.blend(x, y, c, a);
        }
    }
}

const RED: [u8; 3] = [220, 30, 30];
const MAGENTA: [u8; 3] = [200, 0, 200];
const SYS: [[u8; 3]; 2] = [[30, 90, 230], [20, 170, 60]];
const HEADER: [u8; 3] = [240, 150, 0];
const FOOTER: [u8; 3] = [120, 120, 120];

pub fn overlay(g: &Gray, res: &PageResult) -> Rgb {
    let mut img = Rgb::from_gray(g, true);
    let (w, h) = (g.w as i64, g.h as i64);
    let thick = (res.line_thickness_px.round() as i64).max(1);
    // Regions as slanted boxes (deskewed rectangles mapped back).
    let mut sys_i = 0;
    for r in &res.regions {
        let [x0, y0, x1, y1] = r.d;
        let (c, a) = match r.kind {
            Kind::System => {
                sys_i += 1;
                (SYS[sys_i % 2], 0.13)
            }
            Kind::Header => (HEADER, 0.18),
            Kind::Footer => (FOOTER, 0.18),
        };
        for x in x0 as i64..x1 as i64 {
            let ya = unskew_y(res, x as f32, y0) as i64;
            let yb = unskew_y(res, x as f32, y1) as i64;
            for y in ya..yb {
                img.blend(x, y, c, a);
            }
            for k in 0..2 {
                img.put(x, ya + k, c);
                img.put(x, yb - 1 - k, c);
            }
        }
        for y in y0 as i64..y1 as i64 {
            for k in 0..2 {
                let yl = unskew_y(res, x0, y as f32) as i64;
                img.put(x0 as i64 + k, yl, c);
                let yr = unskew_y(res, x1, y as f32) as i64;
                img.put(x1 as i64 - 1 - k, yr, c);
            }
        }
        if r.kind == Kind::System {
            text(&mut img, x0 as i64 + 6, unskew_y(res, x0, y0) as i64 + 6, 4, &r.staves.to_string(), c);
        }
    }
    // Cut lines (full width), dashed when the cut goes through ink.
    for r in res.regions.iter().filter(|r| r.kind == Kind::System) {
        for (y, clean) in [(r.d[1], r.clean_top), (r.d[3], r.clean_bottom)] {
            for x in 0..w {
                if !clean && (x / 12) % 2 == 1 {
                    continue;
                }
                let yy = unskew_y(res, x as f32, y) as i64;
                img.put(x, yy, MAGENTA);
                img.put(x, yy + 1, MAGENTA);
            }
        }
    }
    // Staff lines.
    for s in &res.staves {
        let c = if s.lines == 6 { [230, 120, 0] } else { RED };
        for &ly in &s.line_y {
            for x in s.x0 as i64..s.x1 as i64 {
                let yy = unskew_y(res, x as f32, ly).round() as i64;
                for k in 0..thick {
                    img.put(x, yy - thick / 2 + k, c);
                }
            }
        }
    }
    // Confidence: number + bar, red frame on fallback.
    let conf = format!("{:.2}", res.confidence);
    let bar_c = if res.fallback { RED } else { [20, 160, 40] };
    rect_fill(&mut img, 8, 8, 8 + 200, 8 + 36, [255, 255, 255], 0.9);
    text(&mut img, 14, 12, 5, &conf, bar_c);
    rect_fill(&mut img, 100, 18, 100 + (100.0 * res.confidence) as i64, 34, bar_c, 1.0);
    if res.fallback {
        for k in 0..6 {
            for x in 0..w {
                img.put(x, k, RED);
                img.put(x, h - 1 - k, RED);
            }
            for y in 0..h {
                img.put(k, y, RED);
                img.put(w - 1 - k, y, RED);
            }
        }
    }
    img
}
