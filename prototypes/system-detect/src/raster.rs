//! Rasterisation with hayro and a minimal greyscale / RGB image type.

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::page::Page;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings, render};

/// 8-bit greyscale image, row-major, 0 = black.
#[derive(Clone)]
pub struct Gray {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

/// 8-bit RGB image, row-major.
#[derive(Clone)]
pub struct Rgb {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

/// Page size in PDF points (1/72 inch) as hayro renders it (crop box, rotation applied).
pub fn page_size_pt(page: &Page) -> (f32, f32) {
    page.render_dimensions()
}

fn render_rgba(page: &Page, dpi: f32) -> (usize, usize, Vec<u8>) {
    let scale = dpi / 72.0;
    let px = render(
        page,
        &RenderCache::new(),
        &InterpreterSettings::default(),
        &RenderSettings::default(),
        &PixmapSettings { x_scale: scale, y_scale: scale, bg_color: WHITE },
    );
    (px.width() as usize, px.height() as usize, px.data_as_u8_slice().to_vec())
}

/// Renders a page at `dpi` and converts it to greyscale (background white).
pub fn render_gray(page: &Page, dpi: f32) -> Gray {
    let (w, h, rgba) = render_rgba(page, dpi);
    let px =
        rgba.chunks_exact(4).map(|p| ((p[0] as u32 * 77 + p[1] as u32 * 150 + p[2] as u32 * 29) >> 8) as u8).collect();
    Gray { w, h, px }
}

/// Renders a page at `dpi` in colour (for views).
pub fn render_rgb(page: &Page, dpi: f32) -> Rgb {
    let (w, h, rgba) = render_rgba(page, dpi);
    let px = rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    Rgb { w, h, px }
}

impl Rgb {
    pub fn new(w: usize, h: usize, fill: [u8; 3]) -> Self {
        Rgb { w, h, px: fill.repeat(w * h) }
    }

    pub fn from_gray(g: &Gray, lighten: bool) -> Self {
        let px =
            g.px.iter()
                .flat_map(|&v| {
                    let v = if lighten { 128 + v / 2 } else { v };
                    [v, v, v]
                })
                .collect();
        Rgb { w: g.w, h: g.h, px }
    }

    #[inline]
    pub fn put(&mut self, x: i64, y: i64, c: [u8; 3]) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            let i = (y as usize * self.w + x as usize) * 3;
            self.px[i..i + 3].copy_from_slice(&c);
        }
    }

    /// Blends `c` over the pixel with opacity `a` (0..1).
    #[inline]
    pub fn blend(&mut self, x: i64, y: i64, c: [u8; 3], a: f32) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            let i = (y as usize * self.w + x as usize) * 3;
            for k in 0..3 {
                self.px[i + k] = (self.px[i + k] as f32 * (1.0 - a) + c[k] as f32 * a) as u8;
            }
        }
    }

    /// Copies the rectangle (sx, sy, w, h) of `src` to (dx, dy) of `self`.
    pub fn blit(&mut self, src: &Rgb, sx: usize, sy: usize, w: usize, h: usize, dx: usize, dy: usize) {
        for y in 0..h {
            let (ys, yd) = (sy + y, dy + y);
            if ys >= src.h || yd >= self.h {
                break;
            }
            let w = w.min(src.w.saturating_sub(sx)).min(self.w.saturating_sub(dx));
            let s = (ys * src.w + sx) * 3;
            let d = (yd * self.w + dx) * 3;
            self.px[d..d + w * 3].copy_from_slice(&src.px[s..s + w * 3]);
        }
    }

    /// Box-filter downscale by an integer factor (for thumbnails).
    pub fn downscale(&self, f: usize) -> Rgb {
        let (w, h) = (self.w / f, self.h / f);
        let mut out = Rgb::new(w, h, [255; 3]);
        for y in 0..h {
            for x in 0..w {
                let mut acc = [0u32; 3];
                for yy in 0..f {
                    for xx in 0..f {
                        let i = ((y * f + yy) * self.w + x * f + xx) * 3;
                        for k in 0..3 {
                            acc[k] += self.px[i + k] as u32;
                        }
                    }
                }
                let i = (y * w + x) * 3;
                for k in 0..3 {
                    out.px[i + k] = (acc[k] / (f * f) as u32) as u8;
                }
            }
        }
        out
    }

    pub fn write_png(&self, path: &std::path::Path) -> std::io::Result<()> {
        let file = std::io::BufWriter::new(std::fs::File::create(path)?);
        let mut enc = png::Encoder::new(file, self.w as u32, self.h as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header().map_err(std::io::Error::other)?;
        wr.write_image_data(&self.px).map_err(std::io::Error::other)?;
        Ok(())
    }
}
