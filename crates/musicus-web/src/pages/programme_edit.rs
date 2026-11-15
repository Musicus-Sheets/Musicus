//! Creating and editing concert programmes (librarians and admins), reached
//! from "New programme" on the list and "Edit" on a programme.

use crate::{
    Route, api,
    ui::{ErrorBox, Loading, NoAccess, can_manage, confirm, none_if_empty},
};
use dioxus::prelude::*;
use gloo_net::http::Method;
use musicus_api::{IdList, Programme, ProgrammeInput, Score};

#[component]
pub fn NewProgramme() -> Element {
    rsx! { ProgrammeEditor { id: None } }
}

#[component]
pub fn EditProgramme(id: i64) -> Element {
    rsx! { ProgrammeEditor { key: "{id}", id: Some(id) } }
}

/// Title, date, notes and the pieces in concert order. Saving returns to the
/// programme.
#[component]
fn ProgrammeEditor(id: Option<i64>) -> Element {
    let nav = navigator();
    let programme = use_resource(move || async move {
        match id {
            Some(id) => api::get::<Programme>(&format!("/api/programmes/{id}")).await.map(Some),
            None => Ok(None),
        }
    });
    let scores = use_resource(|| api::get::<Vec<Score>>("/api/scores"));
    let mut title = use_signal(String::new);
    let mut date = use_signal(String::new);
    let mut notes = use_signal(String::new);
    let mut items = use_signal(Vec::<(i64, String)>::new);
    let mut pick = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);
    let mut saving = use_signal(|| false);
    // Set once a new programme was created, so a retry after a failed
    // second request does not create it twice.
    let mut created = use_signal(|| None::<i64>);

    use_effect(move || {
        if let Some(Ok(Some(p))) = &*programme.read() {
            title.set(p.title.clone());
            date.set(p.date.clone().unwrap_or_default());
            notes.set(p.notes.clone().unwrap_or_default());
            items.set(p.items.iter().map(|i| (i.score.id, i.score.title.clone())).collect());
        }
    });
    if !can_manage() {
        return rsx! { NoAccess {} };
    }

    let save = move |e: FormEvent| async move {
        e.prevent_default();
        saving.set(true);
        let input = ProgrammeInput {
            title: title().trim().to_owned(),
            date: none_if_empty(date()),
            notes: none_if_empty(notes()),
        };
        let result = async {
            let pid = match id.or(created()) {
                Some(pid) => {
                    api::put::<_, Programme>(&format!("/api/programmes/{pid}"), &input).await?;
                    pid
                }
                None => {
                    let p = api::post::<_, Programme>("/api/programmes", &input).await?;
                    created.set(Some(p.id));
                    p.id
                }
            };
            let ids = IdList { ids: items().iter().map(|i| i.0).collect() };
            api::put::<_, Programme>(&format!("/api/programmes/{pid}/items"), &ids).await?;
            Ok::<_, api::ApiErr>(pid)
        }
        .await;
        saving.set(false);
        match result {
            Ok(pid) => {
                nav.push(Route::ProgrammeView { id: pid });
            }
            Err(e) => error.set(Some(e.message)),
        }
    };
    let delete = move |_| async move {
        let Some(pid) = id else { return };
        if !confirm("Delete this programme? The pieces stay in the library.") {
            return;
        }
        match api::call(Method::DELETE, &format!("/api/programmes/{pid}")).await {
            Ok(()) => {
                nav.push(Route::Programmes {});
            }
            Err(e) => error.set(Some(e.message)),
        }
    };
    let all_scores = match &*scores.read() {
        Some(Ok(list)) => list.clone(),
        _ => Vec::new(),
    };
    let count = items.read().len();
    let back = match id {
        Some(id) => Route::ProgrammeView { id },
        None => Route::Programmes {},
    };
    rsx! {
        main { class: "medium",
            div { class: "crumbs", Link { to: Route::Programmes {}, "‹ Programmes" } }
            match &*programme.read() {
                None => rsx! { Loading {} },
                Some(Err(e)) => rsx! { ErrorBox { message: e.message.clone() } },
                Some(Ok(_)) => rsx! {
                    h1 { if id.is_some() { "Edit programme" } else { "New programme" } }
                    if let Some(e) = error() { ErrorBox { message: e } }
                    form { onsubmit: save,
                        section { class: "card",
                            h2 { "Concert" }
                            div { class: "grid",
                                label { "Title"
                                    input { required: true, placeholder: "e.g. Christmas Concert 2026", value: "{title}",
                                        oninput: move |e| title.set(e.value()) } }
                                label { "Date" input { r#type: "date", value: "{date}", oninput: move |e| date.set(e.value()) } }
                                label { class: "wide", "Notes for the musicians"
                                    textarea { rows: 2, placeholder: "e.g. Meet at 18:30 for the sound check.", value: "{notes}",
                                        oninput: move |e| notes.set(e.value()) } }
                            }
                        }
                        section { class: "card",
                            h2 { "Pieces in concert order" }
                            if count == 0 { p { class: "muted", "No pieces yet. Add them below in playing order." } }
                            ol { class: "items",
                                for (index, (score_id, name)) in items.read().iter().cloned().enumerate() {
                                    li { key: "{index}-{score_id}",
                                        span { class: "position", "{index + 1}" }
                                        span { class: "grow", "{name}" }
                                        button { r#type: "button", class: "icon-button", disabled: index == 0, title: "Move up",
                                            "aria-label": "Move up", onclick: move |_| items.write().swap(index, index - 1), "↑" }
                                        button { r#type: "button", class: "icon-button", disabled: index + 1 == count, title: "Move down",
                                            "aria-label": "Move down", onclick: move |_| items.write().swap(index, index + 1), "↓" }
                                        button { r#type: "button", class: "icon-button danger", title: "Remove",
                                            "aria-label": "Remove", onclick: move |_| { items.write().remove(index); }, "×" }
                                    }
                                }
                            }
                            div { class: "inline",
                                select { class: "grow", value: "{pick}", onchange: move |e| pick.set(e.value()), "aria-label": "Piece to add",
                                    option { value: "", "Add a piece…" }
                                    for s in all_scores.iter() {
                                        option { key: "{s.id}", value: "{s.id}",
                                            "{s.title}"
                                            if let Some(c) = &s.composer { " – {c}" }
                                        }
                                    }
                                }
                                button {
                                    r#type: "button",
                                    disabled: pick().is_empty(),
                                    onclick: move |_| {
                                        if let Ok(sid) = pick().parse::<i64>()
                                            && let Some(s) = all_scores.iter().find(|s| s.id == sid)
                                        {
                                            items.write().push((s.id, s.title.clone()));
                                            pick.set(String::new());
                                        }
                                    },
                                    "+ Add"
                                }
                            }
                            p { class: "muted small", "A piece may appear more than once, e.g. as an encore." }
                        }
                        div { class: "actions",
                            Link { class: "button", to: back, "Cancel" }
                            span { class: "grow" }
                            button { class: "primary", r#type: "submit", disabled: saving(),
                                if id.is_some() { "Save programme" } else { "Create programme" } }
                        }
                    }
                    if id.is_some() {
                        section { class: "card danger-zone",
                            h2 { "Delete programme" }
                            p { class: "muted small", "Removes only the concert; the pieces stay in the library." }
                            button { class: "danger", onclick: delete, "Delete programme" }
                        }
                    }
                },
            }
        }
    }
}
