//! nav-sim <pdf> [--pages a-b] [--dpi 150] [--staff-mm 6,7,8,10] [--out DIR]
//!           [--render SCREEN:MM[:N]]...
//!
//! Runs detection, then simulates the system-wise reader on several tablet screens and
//! minimum staff heights. Prints a JSON summary; `--render tab8-land:7:6` writes the first 6
//! views of that combination as PNGs (`DIR/<screen>_<mm>mm_vNN.png`).

use hayro::hayro_syntax::Pdf;
use serde::Serialize;
use std::path::PathBuf;
use system_detect::detect::{self, PageResult};
use system_detect::nav::{self, Item, PageGeom, Screen, View};
use system_detect::raster::{self, Rgb};

fn screens() -> Vec<Screen> {
    let s = |name: &str, w, h, ppi| Screen { name: name.into(), w_px: w, h_px: h, ppi };
    vec![
        s("tab8-land", 1280, 800, 189.0),
        s("tab8-port", 800, 1280, 189.0),
        s("tab10.5-land", 2224, 1668, 264.0),
        s("tab10.5-port", 1668, 2224, 264.0),
    ]
}

#[derive(Serialize)]
struct Combo {
    screen: String,
    screen_mm: (f32, f32),
    min_staff_mm: f32,
    pages: usize,
    items: usize,
    systems: usize,
    views: usize,
    mean_systems_per_view: f32,
    views_below_min: usize,
    /// Median staff height on screen with today's full-page fit.
    staff_mm_page_fit: f32,
    /// Median staff height on screen in the system views.
    staff_mm_views: f32,
    /// staff_mm_views / staff_mm_page_fit.
    magnification: f32,
    view_list: Vec<View>,
}

#[derive(Serialize)]
struct Out {
    file: String,
    detected_pages: Vec<(usize, usize, f32, bool)>,
    item_list: Vec<Item>,
    combos: Vec<Combo>,
}

fn median(mut v: Vec<f32>) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut file, mut pages, mut dpi, mut out) = (None, None::<(usize, usize)>, 150.0f32, None::<PathBuf>);
    let mut staff_mm = vec![6.0f32, 7.0, 8.0, 10.0];
    let mut renders: Vec<(String, f32, usize)> = vec![];
    while let Some(a) = args.next() {
        match a.as_str() {
            "--pages" => {
                let v = args.next().unwrap();
                let (a, b) = v.split_once('-').unwrap_or((&v, &v));
                pages = Some((a.parse().unwrap(), b.parse().unwrap()));
            }
            "--dpi" => dpi = args.next().unwrap().parse().unwrap(),
            "--out" => out = Some(args.next().unwrap().into()),
            "--staff-mm" => staff_mm = args.next().unwrap().split(',').map(|s| s.parse().unwrap()).collect(),
            "--render" => {
                let v = args.next().unwrap();
                let p: Vec<&str> = v.split(':').collect();
                renders.push((p[0].into(), p[1].parse().unwrap(), p.get(2).map_or(6, |n| n.parse().unwrap())));
            }
            _ => file = Some(a),
        }
    }
    let file =
        file.expect("usage: nav-sim <pdf> [--pages a-b] [--staff-mm 6,7,8,10] [--out DIR] [--render SCREEN:MM[:N]]");
    let pdf = Pdf::new(std::fs::read(&file).unwrap()).expect("parse pdf");
    let all = pdf.pages();
    let (a, b) = pages.unwrap_or((1, all.len()));
    let b = b.min(all.len());
    let params = detect::Params::default();
    let mut geoms = Vec::new();
    let mut results: Vec<PageResult> = Vec::new();
    for n in a..=b {
        let page = &all[n - 1];
        let (wpt, hpt) = raster::page_size_pt(page);
        let eff = dpi.min(dpi / 72.0 * 842.0 / wpt.max(hpt) * 72.0);
        let g = raster::render_gray(page, eff);
        results.push(detect::detect(&g, &params));
        geoms.push(PageGeom { w_mm: wpt / 72.0 * 25.4, h_mm: hpt / 72.0 * 25.4 });
    }
    let pairs: Vec<(PageGeom, &PageResult)> = geoms.iter().copied().zip(results.iter()).collect();
    let items = nav::items(&pairs);
    let mut combos = Vec::new();
    for sc in screens() {
        for &mm in &staff_mm {
            let views = nav::plan(&items, &geoms, &sc, mm);
            let fit_staff: Vec<f32> = items
                .iter()
                .filter(|it| it.staff_mm > 0.0)
                .map(|it| it.staff_mm * nav::page_fit_scale(geoms[it.page], &sc))
                .collect();
            let view_staff: Vec<f32> =
                views.iter().filter(|v| v.staff_mm_screen > 0.0).map(|v| v.staff_mm_screen).collect();
            let systems = items.iter().filter(|it| it.kind == "system").count();
            let shown: usize =
                views.iter().map(|v| (v.first..=v.last).filter(|&i| items[i].kind == "system").count()).sum();
            let (pf, vs) = (median(fit_staff), median(view_staff));
            combos.push(Combo {
                screen: sc.name.clone(),
                screen_mm: (sc.w_mm(), sc.h_mm()),
                min_staff_mm: mm,
                pages: geoms.len(),
                items: items.len(),
                systems,
                views: views.len(),
                mean_systems_per_view: shown as f32 / views.len().max(1) as f32,
                views_below_min: views.iter().filter(|v| v.below_min).count(),
                staff_mm_page_fit: pf,
                staff_mm_views: vs,
                magnification: if pf > 0.0 { vs / pf } else { 0.0 },
                view_list: views,
            });
        }
    }
    if let Some(dir) = &out {
        std::fs::create_dir_all(dir).unwrap();
        for (scn, mm, n) in &renders {
            let sc = screens().into_iter().find(|s| &s.name == scn).expect("unknown screen");
            let combo =
                combos.iter().find(|c| &c.screen == scn && c.min_staff_mm == *mm).expect("staff mm not simulated");
            for (vi, v) in combo.view_list.iter().take(*n).enumerate() {
                render_view(&all, a, &items, &geoms, &sc, v)
                    .write_png(&dir.join(format!("{scn}_{mm}mm_v{:02}.png", vi + 1)))
                    .unwrap();
            }
        }
    }
    let o = Out {
        file,
        detected_pages: results.iter().enumerate().map(|(i, r)| (a + i, r.systems, r.confidence, r.fallback)).collect(),
        item_list: items,
        combos,
    };
    let json = serde_json::to_string_pretty(&o).unwrap();
    match &out {
        Some(dir) => std::fs::write(dir.join("nav.json"), json).unwrap(),
        None => println!("{json}"),
    }
}

/// Composes one view: the items cropped to the view's content width, stacked from the top.
fn render_view(
    all: &[hayro::hayro_syntax::page::Page],
    first_page: usize,
    items: &[Item],
    geoms: &[PageGeom],
    sc: &Screen,
    v: &View,
) -> Rgb {
    let mut canvas = Rgb::new(sc.w_px as usize, sc.h_px as usize, [255, 255, 255]);
    let px_per_mm = v.scale * sc.ppi / 25.4;
    let its = &items[v.first..=v.last];
    let x0_mm = its.iter().map(|it| it.x0 * geoms[it.page].w_mm).fold(f32::MAX, f32::min);
    let x1_mm = its.iter().map(|it| it.x1 * geoms[it.page].w_mm).fold(0.0, f32::max);
    let content_w = ((x1_mm - x0_mm) * px_per_mm) as usize;
    let dx = (sc.w_px as usize).saturating_sub(content_w) / 2;
    let mut y = 0usize;
    let mut cache: Option<(usize, Rgb)> = None;
    for (k, it) in its.iter().enumerate() {
        if cache.as_ref().is_none_or(|c| c.0 != it.page) {
            let dpi = px_per_mm * 25.4;
            cache = Some((it.page, raster::render_rgb(&all[first_page - 1 + it.page], dpi)));
        }
        let img = &cache.as_ref().unwrap().1;
        let g = geoms[it.page];
        let sx = (x0_mm * px_per_mm) as usize;
        let sy = (it.y0 * g.h_mm * px_per_mm) as usize;
        let h = ((it.y1 - it.y0) * g.h_mm * px_per_mm) as usize;
        canvas.blit(img, sx, sy, content_w, h, dx, y);
        if k > 0 {
            // Separator: blue at a page turn, light grey between systems of one page.
            let c = if its[k - 1].page != it.page { [60, 120, 230] } else { [215, 215, 215] };
            for x in 0..canvas.w {
                canvas.put(x as i64, y as i64, c);
                if c[2] == 230 {
                    canvas.put(x as i64, y as i64 + 1, c);
                }
            }
        }
        y += h;
    }
    canvas
}
