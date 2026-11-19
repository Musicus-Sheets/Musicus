//! Adding and editing pieces, reached from the library by librarians and
//! admins: the guided flow for a new piece (details, then parts with their
//! PDFs), the edit view of a piece and the edit view of a single part.

use crate::{
    Route, api,
    pages::{
        forms::{AddedParts, NewPartForm, PartEditor, ScoreDraft, ScoreFields, Upload},
        library::FOCUS_SCORE,
    },
    ui::{ErrorBox, Loading, NoAccess, Status, can_manage, confirm},
};
use dioxus::prelude::*;
use gloo_net::http::Method;
use musicus_api::{Instrument, Part, Score};
use std::collections::HashMap;

fn instrument_names(list: &[Instrument]) -> HashMap<i64, String> {
    list.iter().map(|i| (i.id, i.name.clone())).collect()
}

/// Opens the library with the given piece highlighted.
fn back_to_library(nav: dioxus::router::Navigator, score_id: Option<i64>) {
    *FOCUS_SCORE.write() = score_id;
    nav.push(Route::Library {});
}

/// Step indicator of the guided flow.
#[component]
fn Steps(current: u8) -> Element {
    let class = |n: u8| {
        if n == current {
            "on"
        } else if n < current {
            "done"
        } else {
            ""
        }
    };
    rsx! {
        ol { class: "steps", "aria-label": "Progress",
            li { class: class(1), span { class: "step-no", if current > 1 { "✓" } else { "1" } } "Details" }
            li { class: class(2), span { class: "step-no", "2" } "Parts and PDFs" }
        }
    }
}

/// Guided flow for a new piece: step 1 creates the score from its details,
/// step 2 adds parts one after another, each with its PDF.
#[component]
pub fn NewScore() -> Element {
    let nav = navigator();
    let mut step = use_signal(|| 1u8);
    let draft = use_signal(ScoreDraft::default);
    let mut score = use_signal(|| None::<Score>);
    let mut rows = use_signal(Vec::<(Part, Upload)>::new);
    let mut error = use_signal(|| None::<String>);
    let instruments = use_resource(|| api::get::<Vec<Instrument>>("/api/instruments"));
    if !can_manage() {
        return rsx! { NoAccess {} };
    }

    let next = move |e: FormEvent| async move {
        e.prevent_default();
        let input = draft().to_input();
        let result = match score() {
            None => api::post::<_, Score>("/api/scores", &input).await,
            Some(s) => api::put::<_, Score>(&format!("/api/scores/{}", s.id), &input).await,
        };
        match result {
            Ok(s) => {
                score.set(Some(s));
                error.set(None);
                step.set(2);
            }
            Err(e) => error.set(Some(e.message)),
        }
    };
    let on_part = move |(part, state): (Part, Upload)| {
        let mut list = rows.write();
        match list.iter_mut().find(|(p, _)| p.id == part.id) {
            Some(row) => row.1 = state,
            None => list.push((part, state)),
        }
    };
    let all = match &*instruments.read() {
        Some(Ok(list)) => list.clone(),
        _ => Vec::new(),
    };
    let uploading = rows.read().iter().any(|(_, s)| matches!(s, Upload::Running(_)));
    rsx! {
        main { class: "medium",
            div { class: "crumbs", Link { to: Route::Library {}, "‹ Library" } }
            h1 { "Add piece" }
            Steps { current: step() }
            if let Some(e) = error() { ErrorBox { message: e } }
            if step() == 1 {
                form { class: "card", onsubmit: next,
                    h2 { "Details" }
                    p { class: "muted small", "Only the title is required. Genre and tags help musicians find the piece." }
                    ScoreFields { draft }
                    div { class: "actions",
                        Link { class: "button", to: Route::Library {}, "Cancel" }
                        span { class: "grow" }
                        button { class: "primary", r#type: "submit", "Next: parts ›" }
                    }
                }
            } else if let Some(s) = score() {
                section { class: "card",
                    h2 { "Parts of “{s.title}”" }
                    p { class: "muted small",
                        "Add one part per sheet: its instruments, key and voices give its name, e.g. “Trumpet in B♭ 1”; "
                        "a custom name is optional. Groups get access by instrument."
                    }
                    if rows.read().is_empty() {
                        p { class: "muted", "No parts yet." }
                    } else {
                        AddedParts { rows: rows(), names: instrument_names(&all) }
                    }
                }
                section { class: "card",
                    h2 { if rows.read().is_empty() { "Add the first part" } else { "Add another part" } }
                    NewPartForm { score_id: s.id, instruments: all.clone(), onstate: on_part }
                }
                div { class: "actions wizard-actions",
                    button { onclick: move |_| step.set(1), "‹ Details" }
                    span { class: "grow" }
                    if uploading { span { class: "muted small", "Waiting for uploads…" } }
                    button {
                        class: "primary",
                        disabled: uploading,
                        onclick: move |_| back_to_library(nav, score().map(|s| s.id)),
                        "Finish"
                    }
                }
            }
        }
    }
}

/// Edit view of a piece: details, all parts, adding parts, deleting.
#[component]
pub fn EditScore(id: i64) -> Element {
    let nav = navigator();
    let mut score = use_resource(move || async move { api::get::<Score>(&format!("/api/scores/{id}")).await });
    let instruments = use_resource(|| api::get::<Vec<Instrument>>("/api/instruments"));
    let mut draft = use_signal(ScoreDraft::default);
    let mut status = use_signal(|| None::<Result<String, String>>);
    let mut error = use_signal(|| None::<String>);
    let mut added = use_signal(|| None::<(Part, Upload)>);

    // Fill the form once the score arrived (not again after part changes).
    let mut filled = use_signal(|| false);
    use_effect(move || {
        if let Some(Ok(s)) = &*score.read()
            && !*filled.peek()
        {
            draft.set(ScoreDraft::from_score(s));
            filled.set(true);
        }
    });
    if !can_manage() {
        return rsx! { NoAccess {} };
    }

    let save = move |e: FormEvent| async move {
        e.prevent_default();
        match api::put::<_, Score>(&format!("/api/scores/{id}"), &draft().to_input()).await {
            Ok(_) => {
                status.set(Some(Ok("Saved".into())));
                score.restart();
            }
            Err(e) => status.set(Some(Err(e.message))),
        }
    };
    let delete = move |_| async move {
        if !confirm("Delete this piece with all its parts? Everyone's annotations on it are deleted too.") {
            return;
        }
        match api::call(Method::DELETE, &format!("/api/scores/{id}")).await {
            Ok(()) => back_to_library(nav, None),
            Err(e) => error.set(Some(e.message)),
        }
    };
    let all = match &*instruments.read() {
        Some(Ok(list)) => list.clone(),
        _ => Vec::new(),
    };
    rsx! {
        main { class: "medium",
            div { class: "crumbs", Link { to: Route::Library {}, "‹ Library" } }
            match &*score.read() {
                None => rsx! { Loading {} },
                Some(Err(e)) => rsx! { ErrorBox { message: e.message.clone() } },
                Some(Ok(s)) => rsx! {
                    div { class: "page-head",
                        div {
                            h1 { "Edit piece" }
                            p { class: "muted", "{s.title}" }
                        }
                        button { class: "primary", onclick: move |_| back_to_library(nav, Some(id)), "Done" }
                    }
                    if let Some(e) = error() { ErrorBox { message: e } }
                    form { class: "card", onsubmit: save, oninput: move |_| status.set(None),
                        h2 { "Details" }
                        ScoreFields { draft }
                        div { class: "actions",
                            button { class: "primary", r#type: "submit", "Save details" }
                            Status { status: status() }
                        }
                    }
                    section { class: "card",
                        h2 { "Parts ({s.parts.len()})" }
                        if s.parts.is_empty() { p { class: "muted", "No parts yet." } }
                        for p in s.parts.iter() {
                            PartEditor { key: "{p.id}", part: p.clone(), instruments: all.clone(),
                                onchange: move |_| score.restart(), ondelete: move |_| score.restart() }
                        }
                    }
                    section { class: "card",
                        h2 { "Add a part" }
                        if let Some(row) = added() {
                            AddedParts { rows: vec![row], names: instrument_names(&all) }
                        }
                        NewPartForm { score_id: id, instruments: all.clone(),
                            onstate: move |row: (Part, Upload)| {
                                let finished = !matches!(row.1, Upload::Running(_));
                                added.set(Some(row));
                                if finished {
                                    score.restart();
                                }
                            } }
                    }
                    section { class: "card danger-zone",
                        h2 { "Delete piece" }
                        p { class: "muted small",
                            "Deletes the piece with all its parts and files from the library. "
                            strong { "Everyone's annotations on it are deleted too." }
                            " Programmes keep their other pieces."
                        }
                        button { class: "danger", onclick: delete, "Delete piece" }
                    }
                },
            }
        }
    }
}

/// Edit view of one part, reached from its tile in the library.
#[component]
pub fn EditPart(id: i64) -> Element {
    let nav = navigator();
    let mut data = use_resource(move || async move {
        let part = api::get::<Part>(&format!("/api/parts/{id}")).await?;
        let score = api::get::<Score>(&format!("/api/scores/{}", part.score_id)).await?;
        Ok::<_, api::ApiErr>((part, score))
    });
    let instruments = use_resource(|| api::get::<Vec<Instrument>>("/api/instruments"));
    if !can_manage() {
        return rsx! { NoAccess {} };
    }
    let all = match &*instruments.read() {
        Some(Ok(list)) => list.clone(),
        _ => Vec::new(),
    };
    rsx! {
        main { class: "medium",
            match &*data.read() {
                None => rsx! { Loading {} },
                Some(Err(e)) => rsx! { ErrorBox { message: e.message.clone() } },
                Some(Ok((part, score))) => {
                    let score_id = score.id;
                    rsx! {
                        div { class: "crumbs",
                            Link { to: Route::Library {}, "‹ Library" }
                            span { class: "muted", " / " }
                            Link { to: Route::EditScore { id: score_id }, "{score.title}" }
                        }
                        div { class: "page-head",
                            div {
                                h1 { "Edit part" }
                                p { class: "muted", "{part.display_name} · {score.title}" }
                            }
                            button { class: "primary", onclick: move |_| back_to_library(nav, Some(score_id)), "Done" }
                        }
                        section { class: "card",
                            PartEditor { key: "{part.id}", part: part.clone(), instruments: all.clone(), versions_open: true,
                                onchange: move |_| data.restart(),
                                ondelete: move |_| { nav.replace(Route::EditScore { id: score_id }); } }
                        }
                        p { class: "muted small",
                            "Other parts, the details of the piece and deleting it: "
                            Link { to: Route::EditScore { id: score_id }, "Edit piece" }
                        }
                    }
                }
            }
        }
    }
}
