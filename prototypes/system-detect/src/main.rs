//! CLI: system-detect <pdf> [--pages a-b] [--dpi 150] [--out DIR]
//!
//! Prints per-page detection results as JSON (stdout) and a timing summary (stderr).
//! With --out, writes `result.json` and overlay PNGs `pNNN.png` into DIR.

use hayro::hayro_syntax::Pdf;
use serde::Serialize;
use std::path::PathBuf;
use std::time::Instant;
use system_detect::{detect, draw, raster};

#[derive(Serialize)]
struct PageOut {
    page: usize,
    width_mm: f32,
    height_mm: f32,
    /// Effective render resolution (lower than requested for oversized pages).
    dpi: f32,
    render_ms: f64,
    detect_ms: f64,
    result: detect::PageResult,
}

#[derive(Serialize)]
struct DocOut {
    file: String,
    pages_total: usize,
    dpi: f32,
    pages: Vec<PageOut>,
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut file = None;
    let (mut pages, mut dpi, mut out) = (None::<(usize, usize)>, 150.0f32, None::<PathBuf>);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--pages" => {
                let v = args.next().expect("--pages a-b");
                let (a, b) = v.split_once('-').unwrap_or((&v, &v));
                pages = Some((a.parse().unwrap(), b.parse().unwrap()));
            }
            "--dpi" => dpi = args.next().unwrap().parse().unwrap(),
            "--out" => out = Some(args.next().unwrap().into()),
            _ => file = Some(a),
        }
    }
    let file = file.expect("usage: system-detect <pdf> [--pages a-b] [--dpi 150] [--out DIR]");
    let data = std::fs::read(&file).expect("read pdf");
    let pdf = Pdf::new(data).expect("hayro could not parse the PDF");
    let all = pdf.pages();
    let (a, b) = pages.unwrap_or((1, all.len()));
    let b = b.min(all.len());
    if let Some(o) = &out {
        std::fs::create_dir_all(o).unwrap();
    }
    let params = detect::Params::default();
    let mut doc = DocOut { file: file.clone(), pages_total: all.len(), dpi, pages: vec![] };
    for n in a..=b {
        let page = &all[n - 1];
        let (wpt, hpt) = raster::page_size_pt(page);
        let t0 = Instant::now();
        // Oversized pages (e.g. facsimile scans declared at 72 dpi) are rendered with the same
        // pixel budget as an A4 page at `dpi`; detection works in pixels of staff space.
        let max_long = dpi / 72.0 * 842.0;
        let eff_dpi = dpi.min(max_long / wpt.max(hpt) * 72.0);
        let gray = raster::render_gray(page, eff_dpi);
        let t1 = Instant::now();
        let res = detect::detect(&gray, &params);
        let t2 = Instant::now();
        let render_ms = (t1 - t0).as_secs_f64() * 1e3;
        let detect_ms = (t2 - t1).as_secs_f64() * 1e3;
        eprintln!(
            "page {n:3}: {} systems, staves {:?}, conf {:.2}{}, skew {:.2}, render {render_ms:.0} ms, detect {detect_ms:.0} ms {:?}",
            res.systems,
            res.regions.iter().filter(|r| r.kind == detect::Kind::System).map(|r| r.staves).collect::<Vec<_>>(),
            res.confidence,
            if res.fallback { " FALLBACK" } else { "" },
            res.skew_deg,
            res.issues
        );
        if let Some(o) = &out {
            draw::overlay(&gray, &res).write_png(&o.join(format!("p{n:03}.png"))).unwrap();
        }
        doc.pages.push(PageOut {
            page: n,
            width_mm: wpt / 72.0 * 25.4,
            height_mm: hpt / 72.0 * 25.4,
            dpi: eff_dpi,
            render_ms,
            detect_ms,
            result: res,
        });
    }
    let json = serde_json::to_string_pretty(&doc).unwrap();
    if let Some(o) = &out {
        std::fs::write(o.join("result.json"), &json).unwrap();
    } else {
        println!("{json}");
    }
    let n = doc.pages.len().max(1) as f64;
    eprintln!(
        "mean per page: render {:.1} ms, detect {:.1} ms",
        doc.pages.iter().map(|p| p.render_ms).sum::<f64>() / n,
        doc.pages.iter().map(|p| p.detect_ms).sum::<f64>() / n
    );
}
