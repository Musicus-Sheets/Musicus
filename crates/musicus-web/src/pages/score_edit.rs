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
use musicus_i18n::t;
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
        ol { class: "steps", "aria-label": t::library_steps(),
            li { class: class(1), span { class: "step-no", if current > 1 { "✓" } else { "1" } } {t::library_step_details()} }
            li { class: class(2), span { class: "step-no", "2" } {t::library_step_parts()} }
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
            div { class: "crumbs", Link { to: Route::Library {}, {t::library_crumb()} } }
            h1 { {t::library_add_piece()} }
            Steps { current: step() }
            if let Some(e) = error() { ErrorBox { message: e } }
            if step() == 1 {
                form { class: "card", onsubmit: next,
                    h2 { {t::library_step_details()} }
                    p { class: "muted small", {t::library_details_hint()} }
                    ScoreFields { draft }
                    div { class: "actions",
                        Link { class: "button", to: Route::Library {}, {t::common_cancel()} }
                        span { class: "grow" }
                        button { class: "primary", r#type: "submit", {t::library_next_parts()} }
                    }
                }
            } else if let Some(s) = score() {
                section { class: "card",
                    h2 { {t::library_parts_of(&s.title)} }
                    p { class: "muted small",
                        {t::library_parts_hint()}
                    }
                    if rows.read().is_empty() {
                        p { class: "muted", {t::library_parts_none()} }
                    } else {
                        AddedParts { rows: rows(), names: instrument_names(&all) }
                    }
                }
                section { class: "card",
                    h2 { if rows.read().is_empty() { {t::library_add_first_part()} } else { {t::library_add_another_part()} } }
                    NewPartForm { score_id: s.id, instruments: all.clone(), onstate: on_part }
                }
                div { class: "actions wizard-actions",
                    button { onclick: move |_| step.set(1), {t::library_back_details()} }
                    span { class: "grow" }
                    if uploading { span { class: "muted small", {t::library_waiting_uploads()} } }
                    button {
                        class: "primary",
                        disabled: uploading,
                        onclick: move |_| back_to_library(nav, score().map(|s| s.id)),
                        {t::library_finish()}
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
                status.set(Some(Ok(t::common_saved())));
                score.restart();
            }
            Err(e) => status.set(Some(Err(e.message))),
        }
    };
    let delete = move |_| async move {
        if !confirm(&t::library_delete_piece_confirm()) {
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
            div { class: "crumbs", Link { to: Route::Library {}, {t::library_crumb()} } }
            match &*score.read() {
                None => rsx! { Loading {} },
                Some(Err(e)) => rsx! { ErrorBox { message: e.message.clone() } },
                Some(Ok(s)) => rsx! {
                    div { class: "page-head",
                        div {
                            h1 { {t::library_edit_piece_title()} }
                            p { class: "muted", "{s.title}" }
                        }
                        button { class: "primary", onclick: move |_| back_to_library(nav, Some(id)), {t::library_done()} }
                    }
                    if let Some(e) = error() { ErrorBox { message: e } }
                    form { class: "card", onsubmit: save, oninput: move |_| status.set(None),
                        h2 { {t::library_step_details()} }
                        ScoreFields { draft }
                        div { class: "actions",
                            button { class: "primary", r#type: "submit", {t::library_save_details()} }
                            Status { status: status() }
                        }
                    }
                    section { class: "card",
                        h2 { {t::library_parts_count(s.parts.len())} }
                        if s.parts.is_empty() { p { class: "muted", {t::library_parts_none()} } }
                        for p in crate::names::sorted(&s.parts) {
                            PartEditor { key: "{p.id}", part: p.clone(), instruments: all.clone(),
                                onchange: move |_| score.restart(), ondelete: move |_| score.restart() }
                        }
                    }
                    section { class: "card",
                        h2 { {t::library_add_part_title()} }
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
                        h2 { {t::library_delete_piece()} }
                        p { class: "muted small",
                            crate::ui::WithStrong { text: t::library_delete_piece_text(crate::ui::MARK), strong: t::library_delete_piece_warning() }
                        }
                        button { class: "danger", onclick: delete, {t::library_delete_piece()} }
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
                            Link { to: Route::Library {}, {t::library_crumb()} }
                            span { class: "muted", " / " }
                            Link { to: Route::EditScore { id: score_id }, "{score.title}" }
                        }
                        div { class: "page-head",
                            div {
                                h1 { {t::library_edit_part_title()} }
                                p { class: "muted", "{crate::names::part_name(&part)} · {score.title}" }
                            }
                            button { class: "primary", onclick: move |_| back_to_library(nav, Some(score_id)), {t::library_done()} }
                        }
                        section { class: "card",
                            PartEditor { key: "{part.id}", part: part.clone(), instruments: all.clone(), versions_open: true,
                                onchange: move |_| data.restart(),
                                ondelete: move |_| { nav.replace(Route::EditScore { id: score_id }); } }
                        }
                        p { class: "muted small",
                            {t::library_edit_part_more()}
                            Link { to: Route::EditScore { id: score_id }, {t::library_edit_piece_title()} }
                        }
                    }
                }
            }
        }
    }
}
