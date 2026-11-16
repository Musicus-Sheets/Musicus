//! Prints the detected system layout of PDF files, for regression checks of
//! the detection against a local set of scores (which must not be committed):
//!
//!     cargo run --release -p musicus-server --example layout_report -- a.pdf b.pdf
//!
//! One line per page: page number, staves per system, confidence, "whole" if
//! the page is shown as a whole page; then the time per page.

use musicus_api::layout::RegionKind;
use std::time::Instant;

fn main() {
    for path in std::env::args().skip(1) {
        let data = std::fs::read(&path).expect("read file");
        let started = Instant::now();
        let pages = musicus_server::layout::analyse_pdf(data).expect("hayro could not read the PDF");
        println!("{path}");
        for (i, p) in pages.iter().enumerate() {
            let staves: Vec<String> =
                p.regions.iter().filter(|r| r.kind == RegionKind::System).map(|r| r.staves.to_string()).collect();
            println!(
                "  {:3}: {:<40} {:.2}{}",
                i + 1,
                if staves.is_empty() { "-".into() } else { staves.join("/") },
                p.confidence,
                if p.full_page { " whole" } else { "" }
            );
        }
        println!("  {:.0} ms per page", started.elapsed().as_secs_f64() * 1e3 / pages.len().max(1) as f64);
    }
}
