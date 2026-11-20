//! The library: every score the user has parts for, with search. Librarians
//! and admins see the same page with an "Add piece" button and "Edit"
//! actions on every piece and part (d-inline-management).

use crate::{
    Route, api,
    ui::{ErrorBox, Loading, can_manage},
};
use dioxus::prelude::*;
use musicus_api::{Instrument, Part, Score};
use musicus_i18n::t;
use std::collections::HashMap;

/// Builds the query string for `/api/scores`.
fn scores_path(q: &str) -> String {
    let q = q.trim();
    if q.is_empty() { "/api/scores".into() } else { format!("/api/scores?q={}", js_sys::encode_uri_component(q)) }
}

/// Piece to highlight and scroll to when the library opens next, set after
/// adding or editing a piece. Memory only.
pub static FOCUS_SCORE: GlobalSignal<Option<i64>> = Signal::global(|| None);

#[component]
pub fn Library() -> Element {
    let manage = can_manage();
    // Taken once: the highlight stays while this page is open.
    let focus = use_hook(|| FOCUS_SCORE.write().take());
    let mut query = use_signal(String::new);
    // Memory only (R-SEC-001): back to "Title" on the next visit.
    let mut by_number = use_signal(|| false);
    let scores = use_resource(move || {
        let path = scores_path(&query());
        async move {
            // Let typing settle before searching.
            gloo_timers::future::TimeoutFuture::new(150).await;
            api::get::<Vec<Score>>(&path).await
        }
    });
    let instruments = use_resource(|| api::get::<Vec<Instrument>>("/api/instruments"));
    // Scroll to the piece that was just added or edited.
    use_effect(move || {
        if let (Some(id), Some(Ok(_))) = (focus, &*scores.read()) {
            document::eval(&format!(
                "requestAnimationFrame(() => document.getElementById('score-{id}')?.scrollIntoView({{block: 'center'}}))"
            ));
        }
    });
    let names: HashMap<i64, String> = match &*instruments.read() {
        Some(Ok(list)) => list.iter().map(|i| (i.id, i.name.clone())).collect(),
        _ => HashMap::new(),
    };
    rsx! {
        main {
            div { class: "page-head",
                h1 { {t::library_title()} }
                div { class: "head-tools",
                    input { class: "search", r#type: "search", placeholder: t::library_search(),
                        value: "{query}", oninput: move |e| query.set(e.value()) }
                    select { class: "sort", "aria-label": t::library_sort(),
                        onchange: move |e| by_number.set(e.value() == "number"), // i18n-ok: option value
                        option { value: "title", selected: !by_number(), {t::library_sort_title()} }
                        option { value: "number", selected: by_number(), {t::library_sort_number()} }
                    }
                    if manage {
                        Link { class: "button", to: Route::Imports {}, title: t::library_import_folder_hint(), {t::library_import_folder()} }
                        Link { class: "button primary add", to: Route::NewScore {},
                            span { class: "plus", "aria-hidden": "true", "+" }
                            {t::library_add_piece()}
                        }
                    }
                }
            }
            match &*scores.read() {
                None => rsx! { Loading {} },
                Some(Err(e)) => rsx! { ErrorBox { message: e.message.clone() } },
                Some(Ok(list)) if list.is_empty() => rsx! {
                    p { class: "muted",
                        if !query().trim().is_empty() {
                            {t::library_nothing_found()}
                        } else if manage {
                            {t::library_empty_manage()}
                        } else {
                            {t::library_empty()}
                        }
                    }
                },
                Some(Ok(list)) => rsx! {
                    div { class: "score-list",
                        for score in sorted(list, by_number()).iter() {
                            ScoreCard { key: "{score.id}", score: score.clone(), names: names.clone(), programme: None,
                                focus: focus == Some(score.id) }
                        }
                    }
                },
            }
        }
    }
}

/// By title, or by archive number ("28" before "304", pieces without a
/// number last, then by title), with the UI language's collation
/// (a-browser-sort; the server's order is only a stable default).
fn sorted(list: &[Score], by_number: bool) -> Vec<Score> {
    use crate::locale::compare;
    let mut list = list.to_vec();
    list.sort_by(|a, b| {
        let by_title = || compare(&a.title, &b.title);
        if !by_number {
            return by_title();
        }
        match (&a.archive_number, &b.archive_number) {
            (Some(x), Some(y)) => compare(x, y).then_with(by_title),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => by_title(),
        }
    });
    list
}

pub fn score_byline(score: &Score) -> String {
    let mut parts = Vec::new();
    if let Some(c) = &score.composer {
        parts.push(c.clone());
    }
    if let Some(a) = &score.arranger {
        parts.push(format!("arr. {a}"));
    }
    parts.join(" · ")
}

pub fn part_route(part: &Part, programme: Option<i64>) -> Route {
    match programme {
        Some(pid) => Route::ProgrammeViewer { pid, id: part.id },
        None => Route::Viewer { id: part.id },
    }
}

/// A piece with its part tiles. Librarians and admins also get "Edit" on the
/// piece and on every part.
#[component]
pub fn ScoreCard(
    score: Score,
    names: HashMap<i64, String>,
    programme: Option<i64>,
    #[props(default)] focus: bool,
) -> Element {
    let manage = can_manage();
    let byline = score_byline(&score);
    let missing = score.parts.iter().filter(|p| p.file.is_none()).count();
    rsx! {
        article { class: if focus { "score focus" } else { "score" }, id: "score-{score.id}",
            div { class: "score-head",
                div { class: "score-title",
                    if let Some(n) = &score.archive_number {
                        span { class: "archive-number", title: t::library_archive_number(), {t::library_archive_number_badge(n)} }
                    }
                    h2 { "{score.title}" }
                    if let Some(sub) = &score.subtitle {
                        span { class: "muted", "{sub}" }
                    }
                    if !byline.is_empty() {
                        p { class: "muted small", "{byline}" }
                    }
                    div { class: "tags",
                        if let Some(g) = &score.genre {
                            span { class: "tag genre", "{g}" }
                        }
                        for t in score.tags.iter() {
                            span { class: "tag", "{t}" }
                        }
                        if manage && score.parts.is_empty() {
                            span { class: "tag warn", {t::library_no_parts()} }
                        } else if manage && missing > 0 {
                            span { class: "tag warn", {t::library_parts_without_pdf(missing)} }
                        }
                    }
                }
                if manage {
                    Link { class: "button edit", to: Route::EditScore { id: score.id }, title: t::library_edit_piece(), span { class: "glyph", "aria-hidden": "true", "✎" } {t::common_edit()} }
                }
            }
            div { class: "parts",
                for part in crate::names::sorted(&score.parts) {
                    PartTile { key: "{part.id}", part: part.clone(), names: names.clone(), programme, manage }
                }
            }
        }
    }
}

#[component]
fn PartTile(part: Part, names: HashMap<i64, String>, programme: Option<i64>, manage: bool) -> Element {
    if !manage {
        return rsx! { PartLink { part, names, programme } };
    }
    let id = part.id;
    rsx! {
        div { class: "part-cell",
            PartLink { part, names, programme }
            Link { class: "button part-edit", to: Route::EditPart { id }, title: t::library_edit_part(), span { class: "glyph", "aria-hidden": "true", "✎" } {t::common_edit()} }
        }
    }
}

/// The tile itself: thumbnail and part name, opening the reader.
#[component]
fn PartLink(part: Part, names: HashMap<i64, String>, programme: Option<i64>) -> Element {
    // A derived name already contains the instruments; list them only under a custom name.
    let instruments: Vec<String> = if part.label.is_some() {
        part.instrument_ids.iter().filter_map(|i| names.get(i).cloned()).collect()
    } else {
        Vec::new()
    };
    let Some(file) = part.file.clone() else {
        return rsx! {
            div { class: "part disabled", title: t::library_no_file(),
                div { class: "thumb empty", "—" }
                span { class: "part-label", {crate::names::part_name(&part)} }
                if !instruments.is_empty() {
                    span { class: "muted small", "{instruments.join(\", \")}" }
                }
            }
        };
    };
    rsx! {
        Link { class: "part", to: part_route(&part, programme),
            if file.has_thumbnail {
                img { class: "thumb", src: api::thumbnail_url(&file.sha256), alt: "", loading: "lazy" }
            } else {
                div { class: "thumb empty", "PDF" } // i18n-ok: file format name
            }
            span { class: "part-label", {crate::names::part_name(&part)} }
            if !instruments.is_empty() {
                span { class: "muted small", "{instruments.join(\", \")}" }
            }
        }
    }
}
