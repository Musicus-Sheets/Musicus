//! Concert programmes. Opening one downloads all its sheets (and their system
//! layouts) into memory, so turning to the next piece on stage needs no
//! network. Librarians and admins also get "New programme" and "Edit" (editor in `programme_edit.rs`).

use crate::{
    Route, api, bridge, layouts,
    pages::library::ScoreCard,
    ui::{ErrorBox, Loading, can_manage, format_date},
};
use dioxus::prelude::*;
use musicus_api::{Instrument, Programme, ProgrammeSummary};
use musicus_i18n::t;
use std::collections::HashMap;

#[component]
pub fn Programmes() -> Element {
    let manage = can_manage();
    let list = use_resource(|| api::get::<Vec<ProgrammeSummary>>("/api/programmes"));
    rsx! {
        main {
            div { class: "page-head",
                h1 { {t::programmes_title()} }
                if manage {
                    Link { class: "button primary add", to: Route::NewProgramme {},
                        span { class: "plus", "aria-hidden": "true", "+" }
                        {t::programmes_new()}
                    }
                }
            }
            match &*list.read() {
                None => rsx! { Loading {} },
                Some(Err(e)) => rsx! { ErrorBox { message: e.message.clone() } },
                Some(Ok(items)) if items.is_empty() => rsx! {
                    p { class: "muted",
                        if manage { {t::programmes_none_manage()} } else { {t::programmes_none()} }
                    }
                },
                Some(Ok(items)) => rsx! {
                    ul { class: "programme-list",
                        for p in items.iter() {
                            li { key: "{p.id}",
                                Link { to: Route::ProgrammeView { id: p.id },
                                    strong { "{p.title}" }
                                    span { class: "muted",
                                        if let Some(d) = &p.date { "{format_date(d)} · " }
                                        {t::programmes_piece_count(p.item_count)}
                                    }
                                }
                            }
                        }
                    }
                },
            }
        }
    }
}

/// Sheet urls of a programme in concert order.
pub fn programme_urls(p: &Programme) -> Vec<String> {
    let mut urls: Vec<String> = Vec::new();
    for item in &p.items {
        for part in &item.score.parts {
            if let Some(f) = &part.file {
                let url = api::file_url(&f.sha256);
                if !urls.contains(&url) {
                    urls.push(url);
                }
            }
        }
    }
    urls
}

#[component]
pub fn ProgrammeView(id: i64) -> Element {
    let manage = can_manage();
    let programme = use_resource(move || async move { api::get::<Programme>(&format!("/api/programmes/{id}")).await });
    let instruments = use_resource(|| api::get::<Vec<Instrument>>("/api/instruments"));
    let mut loaded = use_signal(|| 0usize);
    let mut failed = use_signal(|| 0usize);
    let mut total = use_signal(|| 0usize);

    // Download every sheet of the programme into memory, a few at a time.
    use_effect(move || {
        let Some(Ok(p)) = &*programme.read() else { return };
        let urls = programme_urls(p);
        // Free the memory of sheets from other programmes.
        bridge::retain_only(&urls);
        // System layouts too (small JSON), so the system view works offline on stage.
        let shas: Vec<String> = urls.iter().filter_map(|u| u.strip_prefix("/api/files/").map(str::to_string)).collect();
        layouts::retain_only(&shas);
        spawn(async move {
            for sha in shas {
                let _ = layouts::load(&sha).await;
            }
        });
        total.set(urls.len());
        loaded.set(0);
        failed.set(0);
        spawn(async move {
            for chunk in urls.chunks(3) {
                let results = futures_join(chunk.iter().map(|u| bridge::prefetch(u))).await;
                for r in results {
                    if r.is_ok() {
                        loaded += 1;
                    } else {
                        failed += 1;
                    }
                }
            }
        });
    });

    let names: HashMap<i64, String> = match &*instruments.read() {
        Some(Ok(list)) => list.iter().map(|i| (i.id, i.name.clone())).collect(),
        _ => HashMap::new(),
    };
    rsx! {
        main {
            match &*programme.read() {
                None => rsx! { Loading {} },
                Some(Err(e)) => rsx! { ErrorBox { message: e.message.clone() } },
                Some(Ok(p)) => rsx! {
                    div { class: "page-head",
                        div {
                            h1 { "{p.title}" }
                            if let Some(d) = &p.date { p { class: "muted", "{format_date(d)}" } }
                        }
                        div { class: "head-tools",
                            div { class: "prefetch",
                                if total() == 0 {
                                    span { class: "muted", {t::programmes_no_sheets()} }
                                } else if loaded() == total() {
                                    span { class: "ok", {t::programmes_sheets_ready(total())} }
                                } else {
                                    progress { max: "{total()}", value: "{loaded()}" }
                                    span { class: "muted small", {t::programmes_downloading(loaded(), total())} }
                                }
                                if failed() > 0 {
                                    span { class: "error-text", {t::programmes_failed(failed())} }
                                }
                            }
                            if manage {
                                Link { class: "button edit", to: Route::EditProgramme { id }, span { class: "glyph", "aria-hidden": "true", "✎" } {t::common_edit()} }
                            }
                        }
                    }
                    if let Some(n) = &p.notes { p { class: "notes", "{n}" } }
                    if p.items.is_empty() {
                        p { class: "muted",
                            if manage { {t::programmes_no_pieces_manage()} } else { {t::programmes_no_pieces()} }
                        }
                    }
                    div { class: "score-list",
                        for item in p.items.iter() {
                            div { key: "{item.position}", class: "programme-item",
                                span { class: "position", "{item.position + 1}" }
                                ScoreCard { score: item.score.clone(), names: names.clone(), programme: Some(p.id) }
                            }
                        }
                    }
                },
            }
        }
    }
}

/// Awaits futures whose work already runs (bridge downloads start eagerly),
/// so awaiting them in order still downloads in parallel.
async fn futures_join<F: std::future::Future>(futures: impl Iterator<Item = F>) -> Vec<F::Output> {
    let handles: Vec<F> = futures.collect();
    let mut out = Vec::with_capacity(handles.len());
    for h in handles {
        out.push(h.await);
    }
    out
}
