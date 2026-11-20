//! Librarians correct the detected music systems of a part's current file
//! (R-READ-016): move, add and delete cut lines (which splits and merges
//! systems), set what a band is (system, header, footer, hidden), or show a
//! page whole. A saved correction applies to every member and survives new
//! detection runs (the server keeps it apart from the detection).

use crate::{
    Route, api, bridge, layouts,
    ui::{ErrorBox, Loading, NoAccess, can_manage, confirm},
};
use dioxus::prelude::*;
use gloo_net::http::Method;
use musicus_api::{
    Part,
    layout::{FileLayout, LayoutState, MIN_BAND_HEIGHT, PageCorrection, PageLayout, RegionKind},
};
use musicus_i18n::t;

const CANVAS: &str = "layout-canvas";
const OVERLAY: &str = "layout-overlay";
/// Arrow keys move a focused cut line by this much (0..1 of the page height).
const KEY_STEP: f32 = 0.004;

const KINDS: [RegionKind; 4] = [RegionKind::System, RegionKind::Header, RegionKind::Footer, RegionKind::Hidden];

fn kind_name(k: RegionKind) -> String {
    match k {
        RegionKind::System => t::reader_systems_kind_system(),
        RegionKind::Header => t::reader_systems_kind_header(),
        RegionKind::Footer => t::reader_systems_kind_footer(),
        RegionKind::Hidden => t::reader_systems_kind_hidden(),
    }
}

fn kind_value(k: RegionKind) -> &'static str {
    match k {
        RegionKind::System => "system",
        RegionKind::Header => "header",
        RegionKind::Footer => "footer",
        RegionKind::Hidden => "hidden",
    }
}

/// The page as an editable correction: its bands as cut lines and kinds.
fn draft_of(p: &PageLayout) -> PageCorrection {
    if p.regions.is_empty() {
        return PageCorrection { full_page: p.full_page, bounds: vec![0.0, 1.0], kinds: vec![RegionKind::System] };
    }
    let mut bounds = vec![p.regions[0].y0];
    bounds.extend(p.regions.iter().map(|r| r.y1));
    // Detected bands are contiguous; guard against rounding.
    for i in 1..bounds.len() {
        if bounds[i] < bounds[i - 1] + MIN_BAND_HEIGHT {
            bounds[i] = (bounds[i - 1] + MIN_BAND_HEIGHT).min(1.0);
        }
    }
    PageCorrection { full_page: p.full_page, bounds, kinds: p.regions.iter().map(|r| r.kind).collect() }
}

#[component]
pub fn LayoutEditor(id: i64) -> Element {
    let part = use_resource(move || async move { api::get::<Part>(&format!("/api/parts/{id}")).await });
    if !can_manage() {
        return rsx! { NoAccess {} };
    }
    rsx! {
        main { class: "medium",
            match &*part.read() {
                None => rsx! { Loading {} },
                Some(Err(e)) => rsx! { ErrorBox { message: e.message.clone() } },
                Some(Ok(p)) => match &p.file {
                    None => rsx! { p { {t::reader_systems_no_pdf()} } },
                    Some(f) => rsx! { Editor { key: "{f.sha256}", part: p.clone(), sha256: f.sha256.clone() } },
                },
            }
        }
    }
}

#[component]
fn Editor(part: Part, sha256: String) -> Element {
    let sha = sha256.clone();
    let mut layout = use_resource(move || {
        let sha = sha.clone();
        async move { api::get::<FileLayout>(&format!("/api/files/{sha}/layout")).await }
    });
    let mut page = use_signal(|| 0usize);
    let mut draft = use_signal(|| None::<PageCorrection>);
    let mut dirty = use_signal(|| false);
    let mut status = use_signal(|| None::<Result<String, String>>);
    let mut size = use_signal(|| None::<(f64, f64)>);
    let mut dragging = use_signal(|| None::<usize>);
    let url = api::file_url(&sha256);

    let current: Option<FileLayout> = match &*layout.read() {
        Some(Ok(l)) => Some(l.clone()),
        _ => None,
    };
    let page_count = current.as_ref().map(|l| l.pages.len()).unwrap_or(0);
    let shown = current.as_ref().and_then(|l| l.pages.get(page()).cloned());

    // A new page (or a fresh layout from the server) resets the draft.
    use_effect(move || {
        let p = page();
        if let Some(Ok(l)) = &*layout.read() {
            draft.set(l.pages.get(p).map(draft_of));
            dirty.set(false);
        }
    });

    // Render the page, as wide as the column.
    use_effect({
        let url = url.clone();
        move || {
            let p = page() as u32;
            let url = url.clone();
            spawn(async move {
                let width = web_sys::window()
                    .and_then(|w| w.inner_width().ok())
                    .and_then(|v| v.as_f64())
                    .map(|w| (w - 48.0).min(820.0))
                    .unwrap_or(800.0);
                match bridge::render_page(&url, p, CANVAS, width, 100_000.0, bridge::pixel_ratio()).await {
                    Ok(Some(s)) => size.set(Some(s)),
                    Ok(None) => {}
                    Err(e) => status.set(Some(Err(t::reader_systems_render_failed(e.to_string())))),
                }
            });
        }
    });

    let mut edit = move |f: &dyn Fn(&mut PageCorrection)| {
        if let Some(d) = draft.write().as_mut() {
            f(d);
        }
        dirty.set(true);
        status.set(None);
    };

    let sha_save = sha256.clone();
    let save = move |_| {
        let sha = sha_save.clone();
        let Some(d) = draft() else { return };
        let p = page();
        spawn(async move {
            status.set(Some(Ok(t::common_saving())));
            match api::put::<_, FileLayout>(&format!("/api/files/{sha}/layout/pages/{p}"), &d).await {
                Ok(l) => {
                    layouts::forget(&sha);
                    layout.set(Some(Ok(l)));
                    status.set(Some(Ok(t::reader_systems_saved())));
                }
                Err(e) => status.set(Some(Err(e.message))),
            }
        });
    };
    let sha_reset = sha256.clone();
    let reset = move |_| {
        let sha = sha_reset.clone();
        let p = page();
        spawn(async move {
            match api::call(Method::DELETE, &format!("/api/files/{sha}/layout/pages/{p}")).await {
                Ok(()) => {
                    layouts::forget(&sha);
                    layout.restart();
                    status.set(Some(Ok(t::reader_systems_reset_done())));
                }
                Err(e) => status.set(Some(Err(e.message))),
            }
        });
    };
    let mut go = move |delta: i64| {
        let target = page() as i64 + delta;
        if target < 0 || target >= page_count as i64 {
            return;
        }
        if dirty() && !confirm(&t::reader_systems_discard_confirm()) {
            return;
        }
        page.set(target as usize);
    };

    // Dragging a cut line: the overlay follows the pointer (captured).
    let overlay_y = |client_y: f64| -> Option<f32> {
        let el = web_sys::window()?.document()?.get_element_by_id(OVERLAY)?;
        let r = el.get_bounding_client_rect();
        (r.height() > 0.0).then(|| ((client_y - r.top()) / r.height()) as f32)
    };
    let mut move_bound = move |j: usize, y: f32| {
        edit(&|d: &mut PageCorrection| {
            let lo = if j == 0 { 0.0 } else { d.bounds[j - 1] + MIN_BAND_HEIGHT };
            let hi = if j + 1 == d.bounds.len() { 1.0 } else { d.bounds[j + 1] - MIN_BAND_HEIGHT };
            d.bounds[j] = y.clamp(lo, hi.max(lo));
        });
    };

    let Some(l) = current else {
        return match &*layout.read() {
            Some(Err(e)) => rsx! { ErrorBox { message: e.message.clone() } },
            _ => rsx! { Loading {} },
        };
    };
    let header = rsx! {
        div { class: "crumbs",
            Link { to: Route::EditPart { id: part.id }, {t::reader_systems_crumb()} }
        }
        div { class: "page-head",
            div {
                h1 { {t::reader_systems_title()} }
                p { class: "muted", {t::reader_systems_subtitle(crate::names::part_name(&part))} }
            }
            Link { class: "button", to: Route::Viewer { id: part.id }, {t::library_open()} }
        }
    };
    if l.state != LayoutState::Done {
        return rsx! {
            {header}
            p { class: "muted",
                if l.state == LayoutState::Pending {
                    {t::reader_systems_pending()}
                } else {
                    {t::reader_systems_failed()}
                }
            }
            button { onclick: move |_| layout.restart(), {t::reader_systems_reload()} }
        };
    }
    let d = draft().unwrap_or_default();
    let (w, h) = size().unwrap_or((0.0, 0.0));
    let corrected = shown.as_ref().is_some_and(|p| p.corrected);
    let n_bounds = d.bounds.len();

    rsx! {
        {header}
        p { class: "muted small",
            {t::reader_systems_hint()}
        }
        div { class: "layout-toolbar",
            button { onclick: move |_| go(-1), disabled: page() == 0, {t::reader_systems_previous_page()} }
            span { class: "page-no-dark", {t::reader_systems_page(page() + 1, page_count)} }
            button { onclick: move |_| go(1), disabled: page() + 1 >= page_count, {t::reader_systems_next_page()} }
            span { class: "grow" }
            if corrected {
                span { class: "tag genre", {t::reader_systems_corrected()} }
            }
            if let Some(p) = &shown {
                if !p.corrected {
                    span { class: "muted small", {t::reader_systems_confidence(crate::locale::format_number(p.confidence as f64, 2, 2))} }
                }
            }
        }
        label { class: "check",
            input { r#type: "checkbox", checked: d.full_page,
                onchange: move |e| { let on = e.checked(); edit(&|d: &mut PageCorrection| d.full_page = on); } }
            {t::reader_systems_whole_page()}
        }
        div { class: "layout-page", style: "width: {w}px; height: {h}px",
            canvas { id: CANVAS }
            div {
                id: OVERLAY,
                class: if d.full_page { "layout-overlay whole" } else { "layout-overlay" },
                onpointermove: move |e: PointerEvent| {
                    if let (Some(j), Some(y)) = (dragging(), overlay_y(e.client_coordinates().y)) {
                        move_bound(j, y);
                    }
                },
                onpointerup: move |_| dragging.set(None),
                onpointercancel: move |_| dragging.set(None),
                for (i, k) in d.kinds.iter().copied().enumerate() {
                    div {
                        key: "band-{i}",
                        class: "band {kind_value(k)}",
                        style: "top: {d.bounds[i] * 100.0}%; height: {(d.bounds[i + 1] - d.bounds[i]) * 100.0}%",
                        div { class: "band-tools",
                            select {
                                "aria-label": t::reader_systems_band_kind(i + 1),
                                onchange: move |e| {
                                    let v = e.value();
                                    if let Some(kind) = KINDS.iter().find(|x| kind_value(**x) == v) {
                                        let kind = *kind;
                                        edit(&|d: &mut PageCorrection| d.kinds[i] = kind);
                                    }
                                },
                                for kind in KINDS {
                                    option { value: kind_value(kind), selected: kind == k, {kind_name(kind)} }
                                }
                            }
                            button {
                                "aria-label": t::reader_systems_split_band(i + 1),
                                title: t::reader_systems_split_hint(),
                                onclick: move |_| edit(&|d: &mut PageCorrection| {
                                    let mid = (d.bounds[i] + d.bounds[i + 1]) / 2.0;
                                    if d.bounds[i + 1] - d.bounds[i] >= 2.0 * MIN_BAND_HEIGHT {
                                        d.bounds.insert(i + 1, mid);
                                        let kind = d.kinds[i];
                                        d.kinds.insert(i + 1, kind);
                                    }
                                }),
                                {t::reader_systems_split()}
                            }
                        }
                        span { class: "band-label", "{kind_name(k)}" }
                    }
                }
                for j in 0..n_bounds {
                    div {
                        key: "cut-{j}",
                        class: if dragging() == Some(j) { "cut dragging" } else { "cut" },
                        style: "top: {d.bounds[j] * 100.0}%",
                        div {
                            class: "grip",
                            role: "separator",
                            tabindex: "0",
                            "aria-label": t::reader_systems_cut(j + 1),
                            "aria-valuenow": "{(d.bounds[j] * 1000.0).round() / 10.0}",
                            onpointerdown: move |e: PointerEvent| {
                                e.prevent_default();
                                if let Some(el) = web_sys::window()
                                    .and_then(|w| w.document())
                                    .and_then(|doc| doc.get_element_by_id(OVERLAY))
                                {
                                    let _ = el.set_pointer_capture(e.pointer_id());
                                }
                                dragging.set(Some(j));
                            },
                            onkeydown: move |e: KeyboardEvent| {
                                let step = match e.key() {
                                    Key::ArrowUp => -KEY_STEP,
                                    Key::ArrowDown => KEY_STEP,
                                    _ => return,
                                };
                                e.prevent_default();
                                let y = draft.peek().as_ref().map(|d| d.bounds[j]).unwrap_or(0.0) + step;
                                move_bound(j, y);
                            },
                        }
                        if j > 0 && j + 1 < n_bounds {
                            button {
                                class: "cut-remove",
                                "aria-label": t::reader_systems_remove_cut(j + 1),
                                title: t::reader_systems_remove_cut_hint(),
                                onclick: move |_| edit(&|d: &mut PageCorrection| {
                                    // The joined band keeps "System" if either part was one.
                                    let kind = if d.kinds[j - 1] == RegionKind::System || d.kinds[j] == RegionKind::System {
                                        RegionKind::System
                                    } else {
                                        d.kinds[j - 1]
                                    };
                                    d.bounds.remove(j);
                                    d.kinds.remove(j);
                                    d.kinds[j - 1] = kind;
                                }),
                                "×"
                            }
                        }
                    }
                }
            }
        }
        div { class: "actions layout-actions",
            button { class: "primary", disabled: !dirty(), onclick: save, {t::reader_systems_save()} }
            if corrected {
                button { onclick: reset, {t::reader_systems_reset()} }
            }
            match status() {
                Some(Ok(s)) => rsx! { span { class: "ok small", "{s}" } },
                Some(Err(e)) => rsx! { span { class: "error-text small", "{e}" } },
                None if dirty() => rsx! { span { class: "muted small", {t::reader_systems_unsaved()} } },
                None => rsx! {},
            }
        }
    }
}
