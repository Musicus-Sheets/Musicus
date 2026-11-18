//! Device pixels of the reader's canvases (sharp rendering, R-READ-001).
//!
//! A canvas is only shown sharp if the browser draws its bitmap 1:1 onto the
//! screen. Browsers place a canvas on whole device pixels themselves and paint
//! it `round(left + width) − round(left)` device pixels wide. If that differs
//! from the backing store, however little (1045 backing pixels stretched over
//! 1046 screen pixels), the whole bitmap is resampled, which blurs every staff
//! line.
//!
//! So the reader does not pick a CSS size and round the pixels; it picks a
//! whole number of device pixels ([`device_size`]) and derives the CSS size
//! from them ([`DeviceSize::css_width`]): CSS size × ratio is then whole, and
//! the painted size equals the backing size wherever the canvas lands. The
//! web client applies the same rule in `js/bridge.js::deviceSize` (page view).

/// Upper bound of pixels per canvas. Safari refuses larger canvases (4096 ×
/// 4096); it also bounds memory when a pinch zoom asks for a high ratio.
/// Keep equal to `MAX_CANVAS_PIXELS` in `js/bridge.js`.
pub const MAX_CANVAS_PIXELS: f64 = 16_777_216.0;

/// A canvas size in whole device pixels and the ratio (device pixels per CSS
/// pixel) it is rendered at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeviceSize {
    pub width: u32,
    pub height: u32,
    pub ratio: f64,
}

impl DeviceSize {
    /// CSS width that covers exactly `width` device pixels.
    pub fn css_width(self) -> f64 {
        self.width as f64 / self.ratio
    }

    /// CSS height that covers exactly `height` device pixels.
    pub fn css_height(self) -> f64 {
        self.height as f64 / self.ratio
    }
}

/// The device pixels for a box of about `css_w` × `css_h` CSS pixels at
/// `ratio` (`devicePixelRatio`, times the pinch zoom). Each side is rounded to
/// whole pixels (at least 1). If the canvas would exceed
/// [`MAX_CANVAS_PIXELS`], the ratio is lowered so it fits (the canvas is then
/// stretched, but as little as possible).
pub fn device_size(css_w: f64, css_h: f64, ratio: f64) -> DeviceSize {
    let (css_w, css_h) = (css_w.max(0.0), css_h.max(0.0));
    let mut ratio = if ratio.is_finite() && ratio > 0.0 { ratio } else { 1.0 };
    let area = css_w * css_h * ratio * ratio;
    if area > MAX_CANVAS_PIXELS {
        ratio *= (MAX_CANVAS_PIXELS / area).sqrt();
    }
    let px = |css: f64| ((css * ratio).round() as u32).max(1);
    let size = DeviceSize { width: px(css_w), height: px(css_h), ratio };
    // Rounding up both sides may still overshoot the cap by a few pixels.
    if size.width as f64 * size.height as f64 > MAX_CANVAS_PIXELS {
        let px = |css: f64| ((css * ratio).floor() as u32).max(1);
        return DeviceSize { width: px(css_w), height: px(css_h), ratio };
    }
    size
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_device_pixels_and_matching_css_size() {
        for ratio in [1.0, 1.25, 1.5, 2.0, 2.625, 3.0] {
            for css in [100.0, 522.91, 806.5, 1045.81, 649.53] {
                let s = device_size(css, css * 1.41, ratio);
                assert!((s.width as f64 - css * ratio).abs() <= 0.5, "{css} at {ratio}");
                assert!((s.height as f64 - css * 1.41 * ratio).abs() <= 0.5, "{css} at {ratio}");
                // The CSS size covers exactly the backing pixels: no resampling.
                assert!((s.css_width() * ratio - s.width as f64).abs() < 1e-9);
                assert!((s.css_height() * ratio - s.height as f64).abs() < 1e-9);
                assert_eq!(s.ratio, ratio);
            }
        }
    }

    #[test]
    fn huge_canvases_are_capped() {
        // A page view pinched to 4× on a 2.625 phone.
        let s = device_size(523.0, 740.0, 2.625 * 4.0);
        assert!(s.width as f64 * s.height as f64 <= MAX_CANVAS_PIXELS);
        assert!(s.ratio < 10.5 && s.ratio > 6.0, "{}", s.ratio);
        assert!((s.css_width() - 523.0).abs() < 0.5 / s.ratio + 1e-9);
        // Below the cap nothing changes.
        assert_eq!(device_size(523.0, 740.0, 2.0).ratio, 2.0);
    }

    #[test]
    fn degenerate_input() {
        let s = device_size(0.0, -5.0, f64::NAN);
        assert_eq!((s.width, s.height, s.ratio), (1, 1, 1.0));
    }
}
