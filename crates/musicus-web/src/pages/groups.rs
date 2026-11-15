//! Groups and what they may read: admins create groups and set their
//! members, librarians and admins set the granted instruments and scores.

use crate::{
    api,
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
    let members = use_signal(|| group.member_ids.clone());
    let granted_instruments = use_signal(|| group.instrument_ids.clone());
    let granted_scores = use_signal(|| group.score_ids.clone());
    let mut score_filter = use_signal(String::new);
    let mut message = use_signal(|| None::<Result<String, String>>);

    let save = move |_| async move {
        let result = async {
            if is_admin {
                api::put::<_, Group>(&format!("/api/groups/{id}/members"), &IdList { ids: members() }).await?;
            }
            api::put::<_, Group>(
                &format!("/api/groups/{id}/instrument-grants"),
                &IdList { ids: granted_instruments() },
            )
            .await?;
            api::put::<_, Group>(&format!("/api/groups/{id}/score-grants"), &IdList { ids: granted_scores() }).await
        }
        .await;
        message.set(Some(result.map(|_| "Saved".into()).map_err(|e| e.message)));
        onchange.call(());
    };
    let delete = move |_| async move {
        if confirm("Delete this group? Its members lose the access it granted.") {
            match api::call(Method::DELETE, &format!("/api/groups/{id}")).await {
                Ok(()) => ondelete.call(()),
                Err(e) => message.set(Some(Err(e.message))),
            }
        }
    };
    let toggle = |mut list: Signal<Vec<i64>>, item: i64| {
        move |e: FormEvent| {
            let mut l = list.write();
            l.retain(|x| *x != item);
            if e.checked() {
                l.push(item);
            }
        }
    };
    let needle = score_filter().to_lowercase();
    rsx! {
        section { class: "card",
            h2 { "{group.name}" }
            p { class: "muted small",
                "Members can read every part tagged with a granted instrument, plus every part of a granted score."
            }
            if is_admin {
                h3 { "Members" }
                div { class: "checks",
                    for u in users.iter() {
                        label { key: "{u.id}", class: "check",
                            input { r#type: "checkbox", checked: members.read().contains(&u.id), onchange: toggle(members, u.id) }
                            "{u.display_name}"
                        }
                    }
                }
            }
            h3 { "Instruments" }
            div { class: "checks",
                for i in instruments.iter() {
                    label { key: "{i.id}", class: "check",
                        input { r#type: "checkbox", checked: granted_instruments.read().contains(&i.id), onchange: toggle(granted_instruments, i.id) }
                        "{i.name}"
                    }
                }
            }
            h3 { "Whole scores (e.g. for the conductor)" }
            input { class: "search", r#type: "search", placeholder: "Filter scores…", value: "{score_filter}",
                oninput: move |e| score_filter.set(e.value()) }
            div { class: "checks scroll",
                for s in scores.iter().filter(|s| needle.is_empty() || s.title.to_lowercase().contains(&needle) || granted_scores.read().contains(&s.id)) {
                    label { key: "{s.id}", class: "check",
                        input { r#type: "checkbox", checked: granted_scores.read().contains(&s.id), onchange: toggle(granted_scores, s.id) }
                        "{s.title}"
                    }
                }
            }
            div { class: "actions",
                button { class: "primary", onclick: save, "Save" }
                Status { status: message() }
                if is_admin {
                    button { class: "danger", onclick: delete, "Delete group" }
                }
            }
        }
    }
}
