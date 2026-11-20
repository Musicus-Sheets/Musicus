//! The instrument list of the ensemble, grouped by family. Everybody can see
//! it; librarians and admins can add, edit and delete instruments here.

use crate::{
    api,
    suggest::SuggestInput,
    ui::{ErrorBox, Loading, can_manage, confirm, none_if_empty},
};
use dioxus::prelude::*;
use gloo_net::http::Method;
use musicus_api::{ErrorCode, Instrument, InstrumentInput};
use std::collections::BTreeMap;

/// Instruments by family; families in alphabetical order, those without a
/// family last.
fn by_family(list: &[Instrument]) -> Vec<(String, Vec<Instrument>)> {
    let mut map: BTreeMap<(bool, String), Vec<Instrument>> = BTreeMap::new();
    for i in list {
        let key = match &i.family {
            Some(f) => (false, f.clone()),
            None => (true, "Other".to_owned()),
        };
        map.entry(key).or_default().push(i.clone());
    }
    map.into_iter().map(|((_, name), items)| (name, items)).collect()
}

#[component]
pub fn Instruments() -> Element {
    let manage = can_manage();
    let mut list = use_resource(|| api::get::<Vec<Instrument>>("/api/instruments"));
    let mut name = use_signal(String::new);
    let family = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);
    let mut added = use_signal(|| None::<String>);

    let add = move |e: FormEvent| async move {
        e.prevent_default();
        let input = InstrumentInput { name: name().trim().to_owned(), family: none_if_empty(family()) };
        match api::post::<_, Instrument>("/api/instruments", &input).await {
            Ok(i) => {
                added.set(Some(i.name));
                name.set(String::new());
                error.set(None);
                list.restart();
            }
            Err(e) => error.set(Some(e.message)),
        }
    };
    let families: Vec<String> = match &*list.read() {
        Some(Ok(items)) => {
            let mut f: Vec<String> = items.iter().filter_map(|i| i.family.clone()).collect();
            f.sort_by_key(|s| s.to_lowercase());
            f.dedup_by_key(|s| s.to_lowercase());
            f
        }
        _ => Vec::new(),
    };
    rsx! {
        main { class: "medium",
            div { class: "page-head", h1 { "Instruments" } }
            p { class: "muted",
                "The instruments of the ensemble. Every part is tagged with the instruments it is for, "
                "and groups get access to the parts by instrument."
            }
            if let Some(e) = error() { ErrorBox { message: e } }
            if manage {
                form { class: "card", onsubmit: add, oninput: move |_| added.set(None),
                    h2 { "Add an instrument" }
                    div { class: "inline",
                        input { placeholder: "Instrument, e.g. Trumpet", required: true, value: "{name}",
                            "aria-label": "Instrument", oninput: move |e| name.set(e.value()) }
                        SuggestInput { value: family, options: families.clone(), placeholder: "Family, e.g. Brass", label: "Family" }
                        button { class: "primary", r#type: "submit", "+ Add" }
                    }
                    if let Some(n) = added() { span { class: "ok", role: "status", "Added “{n}”." } }
                }
            }
            match &*list.read() {
                None => rsx! { Loading {} },
                Some(Err(e)) => rsx! { ErrorBox { message: e.message.clone() } },
                Some(Ok(items)) if items.is_empty() => rsx! {
                    p { class: "muted",
                        if manage { "No instruments yet. Add the instruments of your ensemble above." } else { "No instruments yet." }
                    }
                },
                Some(Ok(items)) => rsx! {
                    for (fam, members) in by_family(items) {
                        section { key: "{fam}", class: "card",
                            h2 { "{fam}" }
                            ul { class: "plain instrument-list",
                                for i in members {
                                    InstrumentRow { key: "{i.id}", instrument: i, manage, families: families.clone(),
                                        onchange: move |_| { error.set(None); list.restart(); },
                                        onerror: move |m| error.set(Some(m)) }
                                }
                            }
                        }
                    }
                },
            }
        }
    }
}

#[component]
fn InstrumentRow(
    instrument: Instrument,
    manage: bool,
    /// Known families, suggested while typing.
    families: Vec<String>,
    onchange: EventHandler<()>,
    onerror: EventHandler<String>,
) -> Element {
    let id = instrument.id;
    let mut editing = use_signal(|| false);
    let mut name = use_signal(|| instrument.name.clone());
    let mut family = use_signal(|| instrument.family.clone().unwrap_or_default());

    let save = move |e: FormEvent| async move {
        e.prevent_default();
        let input = InstrumentInput { name: name().trim().to_owned(), family: none_if_empty(family()) };
        match api::put::<_, Instrument>(&format!("/api/instruments/{id}"), &input).await {
            Ok(_) => {
                editing.set(false);
                onchange.call(());
            }
            Err(e) => onerror.call(e.message),
        }
    };
    let label = instrument.name.clone();
    let delete = move |_| {
        let label = label.clone();
        async move {
            if !confirm(&format!("Delete the instrument “{label}”?")) {
                return;
            }
            match api::call(Method::DELETE, &format!("/api/instruments/{id}")).await {
                Ok(()) => onchange.call(()),
                Err(e) if e.code == Some(ErrorCode::InUseOrUnknown) => onerror.call(format!(
                    "“{label}” cannot be deleted because parts still use it. Tag those parts with another instrument first."
                )),
                Err(e) => onerror.call(e.message),
            }
        }
    };
    if editing() {
        let original = instrument.clone();
        return rsx! {
            li { class: "row editing",
                form { class: "inline grow", onsubmit: save,
                    input { required: true, value: "{name}", "aria-label": "Instrument", oninput: move |e| name.set(e.value()) }
                    SuggestInput { value: family, options: families.clone(), placeholder: "Family", label: "Family" }
                    button { class: "primary", r#type: "submit", "Save" }
                    button {
                        r#type: "button",
                        onclick: move |_| {
                            name.set(original.name.clone());
                            family.set(original.family.clone().unwrap_or_default());
                            editing.set(false);
                        },
                        "Cancel"
                    }
                }
            }
        };
    }
    rsx! {
        li { class: "row",
            span { class: "grow", strong { "{instrument.name}" } }
            if manage {
                button { onclick: move |_| editing.set(true), "Edit" }
                button { class: "danger", onclick: delete, "Delete" }
            }
        }
    }
}
