//! Creating and editing concert programmes (librarians and admins), reached
//! from "New programme" on the list and "Edit" on a programme.

use crate::{
    Route, api,
    picker::{Choice, Picker},
    ui::{ErrorBox, Loading, NoAccess, can_manage, confirm, none_if_empty},
};
use dioxus::prelude::*;
use gloo_net::http::Method;
use musicus_api::{IdList, Programme, ProgrammeInput, Score};
use musicus_i18n::t;

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
        if !confirm(&t::programmes_delete_confirm()) {
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
    let in_programme: Vec<i64> = items.read().iter().map(|i| i.0).collect();
    // Every piece stays choosable (encores); the ones already in the programme say so.
    let piece_choices: Vec<Choice> = all_scores
        .iter()
        .map(|s| {
            let mut detail = s.composer.clone().unwrap_or_default();
            if in_programme.contains(&s.id) {
                detail = if detail.is_empty() {
                    "already in the programme".into()
                } else {
                    format!("{detail} · already in the programme")
                };
            }
            let extra = [s.subtitle.as_deref(), s.arranger.as_deref(), s.archive_number.as_deref()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ");
            Choice::new(s.id, s.title.clone(), detail, &extra)
        })
        .collect();
    let count = items.read().len();
    let back = match id {
        Some(id) => Route::ProgrammeView { id },
        None => Route::Programmes {},
    };
    rsx! {
        main { class: "medium",
            div { class: "crumbs", Link { to: Route::Programmes {}, {t::programmes_crumb()} } }
            match &*programme.read() {
                None => rsx! { Loading {} },
                Some(Err(e)) => rsx! { ErrorBox { message: e.message.clone() } },
                Some(Ok(_)) => rsx! {
                    h1 { if id.is_some() { {t::programmes_edit_title()} } else { {t::programmes_new()} } }
                    if let Some(e) = error() { ErrorBox { message: e } }
                    form { onsubmit: save,
                        section { class: "card",
                            h2 { {t::programmes_concert()} }
                            div { class: "grid",
                                label { {t::programmes_field_title()}
                                    input { required: true, placeholder: t::programmes_title_placeholder(), value: "{title}",
                                        oninput: move |e| title.set(e.value()) } }
                                label { {t::programmes_field_date()} input { r#type: "date", value: "{date}", oninput: move |e| date.set(e.value()) } }
                                label { class: "wide", {t::programmes_field_notes()}
                                    textarea { rows: 2, placeholder: t::programmes_notes_placeholder(), value: "{notes}",
                                        oninput: move |e| notes.set(e.value()) } }
                            }
                        }
                        section { class: "card",
                            h2 { {t::programmes_pieces()} }
                            if count == 0 { p { class: "muted", {t::programmes_pieces_none()} } }
                            ol { class: "items",
                                for (index, (score_id, name)) in items.read().iter().cloned().enumerate() {
                                    li { key: "{index}-{score_id}",
                                        span { class: "position", "{index + 1}" }
                                        span { class: "grow", "{name}" }
                                        button { r#type: "button", class: "icon-button", disabled: index == 0, title: t::programmes_move_up(),
                                            "aria-label": t::programmes_move_up(), onclick: move |_| items.write().swap(index, index - 1), "↑" }
                                        button { r#type: "button", class: "icon-button", disabled: index + 1 == count, title: t::programmes_move_down(),
                                            "aria-label": t::programmes_move_down(), onclick: move |_| items.write().swap(index, index + 1), "↓" }
                                        button { r#type: "button", class: "icon-button danger", title: t::programmes_remove(),
                                            "aria-label": t::programmes_remove(), onclick: move |_| { items.write().remove(index); }, "×" }
                                    }
                                }
                            }
                            Picker {
                                options: piece_choices,
                                label: t::programmes_add_pieces(),
                                placeholder: t::common_search_pieces(),
                                none_left: t::programmes_library_empty(),
                                onapply: move |chosen: Vec<i64>| {
                                    let mut list = items.write();
                                    for sid in chosen {
                                        if let Some(s) = all_scores.iter().find(|s| s.id == sid) {
                                            list.push((s.id, s.title.clone()));
                                        }
                                    }
                                },
                            }
                            p { class: "muted small", {t::programmes_add_hint()} }
                        }
                        div { class: "actions",
                            Link { class: "button", to: back, {t::common_cancel()} }
                            span { class: "grow" }
                            button { class: "primary", r#type: "submit", disabled: saving(),
                                if id.is_some() { {t::programmes_save()} } else { {t::programmes_create()} } }
                        }
                    }
                    if id.is_some() {
                        section { class: "card danger-zone",
                            h2 { {t::programmes_delete()} }
                            p { class: "muted small", {t::programmes_delete_text()} }
                            button { class: "danger", onclick: delete, {t::programmes_delete()} }
                        }
                    }
                },
            }
        }
    }
}
