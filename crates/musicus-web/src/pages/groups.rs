//! Groups and what they may read: admins create groups and set their
//! members, librarians and admins set the granted instruments and scores.
//! Each list is an assignment picker saved immediately (a-assign-picker).

use crate::{
    api,
    picker::{AssignList, Choice},
    ui::{ErrorBox, Loading, NoAccess, Status, can_manage, confirm, is_admin},
};
use dioxus::prelude::*;
use gloo_net::http::Method;
use musicus_api::{Group, GroupInput, IdList, Instrument, Score, UserSummary};

#[component]
pub fn Groups() -> Element {
    let is_admin = is_admin();
    let manage = can_manage();
    let mut groups = use_resource(|| api::get::<Vec<Group>>("/api/groups"));
    let instruments = use_resource(|| api::get::<Vec<Instrument>>("/api/instruments"));
    let scores = use_resource(|| api::get::<Vec<Score>>("/api/scores"));
    let users = use_resource(move || async move {
        if is_admin { api::get::<Vec<UserSummary>>("/api/users").await } else { Ok(Vec::new()) }
    });
    let mut name = use_signal(String::new);
    let mut selected = use_signal(|| None::<i64>);
    let mut error = use_signal(|| None::<String>);

    let create = move |e: FormEvent| async move {
        e.prevent_default();
        match api::post::<_, Group>("/api/groups", &GroupInput { name: name() }).await {
            Ok(g) => {
                name.set(String::new());
                selected.set(Some(g.id));
                groups.restart();
            }
            Err(e) => error.set(Some(e.message)),
        }
    };
    if !manage {
        return rsx! { NoAccess {} };
    }
    let list = match &*groups.read() {
        Some(Ok(l)) => l.clone(),
        Some(Err(e)) => return rsx! { main { ErrorBox { message: e.message.clone() } } },
        None => return rsx! { main { Loading {} } },
    };
    let current = selected().and_then(|id| list.iter().find(|g| g.id == id).cloned());
    rsx! {
        main {
            div { class: "page-head", h1 { "Groups" } }
            p { class: "muted",
                "Groups bundle musicians who read the same music. Members of a group can read every part "
                "tagged with a granted instrument, plus every part of a granted score."
                if !is_admin { " Only admins can create groups and change their members." }
            }
            if let Some(e) = error() { ErrorBox { message: e } }
            div { class: "columns narrow-left",
                section { class: "card",
                    h2 { "Groups" }
                    ul { class: "plain selectable",
                        for g in list.iter() {
                            li { key: "{g.id}", class: if selected() == Some(g.id) { "row on" } else { "row" },
                                onclick: { let id = g.id; move |_| selected.set(Some(id)) },
                                span { "{g.name}" }
                                span { class: "muted small", "{g.member_ids.len()} members" }
                            }
                        }
                    }
                    if is_admin {
                        form { class: "inline", onsubmit: create,
                            input { placeholder: "New group, e.g. Trumpets", required: true, value: "{name}", oninput: move |e| name.set(e.value()) }
                            button { r#type: "submit", "Add" }
                        }
                    }
                }
                match current {
                    None => rsx! { section { class: "card muted", "Select a group to edit its members and access." } },
                    Some(g) => rsx! {
                        GroupEditor {
                            key: "{g.id}",
                            group: g,
                            is_admin,
                            users: match &*users.read() { Some(Ok(u)) => u.clone(), _ => Vec::new() },
                            instruments: match &*instruments.read() { Some(Ok(i)) => i.clone(), _ => Vec::new() },
                            scores: match &*scores.read() { Some(Ok(s)) => s.clone(), _ => Vec::new() },
                            onchange: move |_| groups.restart(),
                            ondelete: move |_| { selected.set(None); groups.restart(); },
                        }
                    },
                }
            }
        }
    }
}

#[component]
fn GroupEditor(
    group: Group,
    is_admin: bool,
    users: Vec<UserSummary>,
    instruments: Vec<Instrument>,
    scores: Vec<Score>,
    onchange: EventHandler<()>,
    ondelete: EventHandler<()>,
) -> Element {
    let id = group.id;
    let mut message = use_signal(|| None::<Result<String, String>>);
    let delete = move |_| async move {
        if confirm("Delete this group? Its members lose the access it granted.") {
            match api::call(Method::DELETE, &format!("/api/groups/{id}")).await {
                Ok(()) => ondelete.call(()),
                Err(e) => message.set(Some(Err(e.message))),
            }
        }
    };
    let people: Vec<Choice> =
        users.iter().map(|u| Choice::new(u.id, u.display_name.clone(), u.username.clone(), "")).collect();
    let instrument_choices: Vec<Choice> = instruments
        .iter()
        .map(|i| Choice::new(i.id, i.name.clone(), i.family.clone().unwrap_or_default(), ""))
        .collect();
    let score_choices: Vec<Choice> = scores
        .iter()
        .map(|s| {
            let extra = [s.subtitle.as_deref(), s.arranger.as_deref(), s.archive_number.as_deref()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ");
            Choice::new(s.id, s.title.clone(), s.composer.clone().unwrap_or_default(), &extra)
        })
        .collect();
    rsx! {
        section { class: "card",
            h2 { "{group.name}" }
            p { class: "muted small",
                "Members can read every part tagged with a granted instrument, plus every part of a granted score. "
                "Changes are saved right away."
            }
            if is_admin {
                h3 { "Members" }
                SavedList {
                    path: format!("/api/groups/{id}/members"),
                    initial: group.member_ids.clone(),
                    options: people,
                    add_label: "+ Add members",
                    empty: "No members yet.",
                    placeholder: "Search name or username…",
                    onchange,
                }
            }
            h3 { "Instruments" }
            SavedList {
                path: format!("/api/groups/{id}/instrument-grants"),
                initial: group.instrument_ids.clone(),
                options: instrument_choices,
                add_label: "+ Add instruments",
                empty: "No instruments granted yet.",
                placeholder: "Search instrument or family…",
                onchange,
            }
            h3 { "Whole scores (e.g. for the conductor)" }
            SavedList {
                path: format!("/api/groups/{id}/score-grants"),
                initial: group.score_ids.clone(),
                options: score_choices,
                add_label: "+ Add scores",
                empty: "No whole scores granted.",
                placeholder: "Search title or composer…",
                onchange,
            }
            if is_admin {
                div { class: "actions",
                    Status { status: message() }
                    span { class: "grow" }
                    button { class: "danger", onclick: delete, "Delete group" }
                }
            } else if message().is_some() {
                Status { status: message() }
            }
        }
    }
}

/// One assignment list of a group, saved with `PUT path {ids}` as soon as
/// the picker closes or an entry is removed; restored when saving fails.
#[component]
fn SavedList(
    path: String,
    initial: Vec<i64>,
    options: Vec<Choice>,
    add_label: String,
    empty: String,
    placeholder: String,
    onchange: EventHandler<()>,
) -> Element {
    let mut ids = use_signal(|| initial.clone());
    let mut busy = use_signal(|| false);
    let mut status = use_signal(|| None::<Result<String, String>>);
    let path = use_signal(|| path.clone());
    let mut generation = use_signal(|| 0u32);
    let store = use_callback(move |next: Vec<i64>| {
        let before = ids();
        ids.set(next.clone());
        busy.set(true);
        generation.set(generation() + 1);
        status.set(Some(Ok("Saving…".into())));
        spawn(async move {
            let result = api::put::<_, Group>(&path(), &IdList { ids: next }).await;
            busy.set(false);
            match result {
                Ok(_) => {
                    status.set(Some(Ok("Saved".into())));
                    onchange.call(());
                    let mine = generation() + 1;
                    generation.set(mine);
                    gloo_timers::future::TimeoutFuture::new(2500).await;
                    if generation() == mine {
                        status.set(None);
                    }
                }
                Err(e) => {
                    ids.set(before);
                    generation.set(generation() + 1);
                    status.set(Some(Err(e.message)));
                }
            }
        });
    });
    rsx! {
        AssignList {
            options,
            assigned: ids(),
            add_label,
            empty,
            placeholder,
            busy: busy(),
            status: status(),
            onadd: move |added: Vec<i64>| {
                let mut next = ids();
                next.extend(added.into_iter().filter(|a| !next.contains(a)).collect::<Vec<_>>());
                store.call(next);
            },
            onremove: move |gone: i64| {
                let next = ids().into_iter().filter(|x| *x != gone).collect();
                store.call(next);
            },
        }
    }
}
