//! Prototype: automatic detection of music systems in sheet-music pages.
//!
//! `detect` is independent of PDF and rendering (input: greyscale raster), so it could move
//! into `musicus-server` as a processor step. `raster` wraps hayro, `draw` makes overlays,
//! `nav` simulates the system-wise reader views.

pub mod detect;
pub mod draw;
pub mod nav;
pub mod raster;
