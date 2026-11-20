//! Suggestions while typing (a-tag-suggest): a small list under a text
//! input with the known values that match what is typed. Used for the
//! tags of a piece (`tag_input.rs`) and the family of an instrument
//! (`SuggestInput`), replacing the browser's `<datalist>`, whose matching
//! and keys differ between browsers and which cannot ignore diacritics.
//!
//! Matching ignores case and diacritics; values starting with the text come
//! first, then values with a word starting with it, then other values
//! containing it. Keys: ↓ / ↑ choose, Enter or Tab take the chosen value,
//! Escape closes the list; a click or tap takes a value. Nothing is chosen
//! until ↓ / ↑ is pressed, so Enter keeps its normal meaning.

use crate::picker::{fold, unique_id};
use dioxus::html::Key;
use dioxus::prelude::*;

/// At most this many suggestions are shown.
const MAX: usize = 8;

/// Known values matching `query`, best first, without those in `exclude`
/// (ignoring case). Empty query: none.
pub fn suggestions(all: &[String], query: &str, exclude: &[String]) -> Vec<String> {
    let q = fold(query.trim());
    if q.is_empty() {
        return Vec::new();
    }
    let excluded: Vec<String> = exclude.iter().map(|e| e.to_lowercase()).collect();
    let mut ranked: Vec<(u8, &String)> = all
        .iter()
        .filter(|v| !excluded.contains(&v.to_lowercase()))
        .filter_map(|v| {
            let f = fold(v);
            if f.starts_with(&q) {
                Some((0, v))
            } else if f.split(|c: char| !c.is_alphanumeric()).any(|w| w.starts_with(&q)) {
                Some((1, v))
            } else if f.contains(&q) {
                Some((2, v))
            } else {
                None
            }
        })
        .collect();
    // Within a rank in the UI language's order (a-browser-sort).
    ranked.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| crate::locale::compare(a.1, b.1)));
    ranked.into_iter().take(MAX).map(|(_, v)| v.clone()).collect()
}

/// Handles the suggestion keys of the input that owns the list. Returns
/// `Some(value)` when a suggestion was taken; `handled` tells the caller
/// that the key was used and must not do anything else.
pub fn on_key(
    e: &KeyboardEvent,
    items: &[String],
    mut open: Signal<bool>,
    mut active: Signal<Option<usize>>,
) -> (bool, Option<String>) {
    let shown = open() && !items.is_empty();
    match e.key() {
        Key::ArrowDown if shown => {
            e.prevent_default();
            active.set(Some(active().map_or(0, |i| (i + 1) % items.len())));
            (true, None)
        }
        Key::ArrowUp if shown => {
            e.prevent_default();
            active.set(Some(active().map_or(items.len() - 1, |i| (i + items.len() - 1) % items.len())));
            (true, None)
        }
        Key::Enter | Key::Tab if shown && active().is_some_and(|i| i < items.len()) => {
            e.prevent_default();
            let value = items[active().unwrap_or(0)].clone();
            open.set(false);
            active.set(None);
            (true, Some(value))
        }
        Key::Escape if shown => {
            e.prevent_default();
            e.stop_propagation();
            open.set(false);
            active.set(None);
            (true, None)
        }
        _ => (false, None),
    }
}

/// The list itself, positioned under the input by the caller's CSS
/// (`.suggest` inside a positioned parent).
#[component]
pub fn SuggestList(
    list_id: String,
    items: Vec<String>,
    active: Option<usize>,
    onpick: EventHandler<String>,
) -> Element {
    rsx! {
        ul { id: "{list_id}", class: "suggest", role: "listbox",
            for (i, v) in items.into_iter().enumerate() {
                li {
                    key: "{v}",
                    id: "{list_id}-{i}",
                    role: "option",
                    class: if active == Some(i) { "active" } else { "" },
                    "aria-selected": if active == Some(i) { "true" } else { "false" },
                    // Keeps the focus (and the typed text) in the input.
                    onmousedown: move |e| e.prevent_default(),
                    onclick: {
                        let v = v.clone();
                        move |e: MouseEvent| {
                            e.stop_propagation();
                            onpick.call(v.clone());
                        }
                    },
                    "{v}"
                }
            }
        }
    }
}

/// A text input with suggestions from `options`; taking one replaces the
/// value. Used for the family of an instrument.
#[component]
pub fn SuggestInput(
    value: Signal<String>,
    options: Vec<String>,
    placeholder: String,
    label: String,
    #[props(default)] required: bool,
) -> Element {
    let mut value = value;
    let uid = use_hook(unique_id);
    let list_id = format!("suggest-{uid}");
    let mut open = use_signal(|| false);
    let mut active = use_signal(|| None::<usize>);
    let items = suggestions(&options, &value.read(), &[]);
    // Nothing to suggest when the value is exactly the only match.
    let items: Vec<String> = if items.len() == 1 && items[0] == *value.read() { Vec::new() } else { items };
    let shown = open() && !items.is_empty();
    let keys = items.clone();
    rsx! {
        div { class: "suggest-wrap",
            input {
                value: "{value}",
                placeholder: "{placeholder}",
                required,
                autocomplete: "off",
                role: "combobox",
                "aria-label": "{label}",
                "aria-autocomplete": "list",
                "aria-expanded": if shown { "true" } else { "false" },
                "aria-controls": "{list_id}",
                "aria-activedescendant": match active() { Some(i) if shown => format!("{list_id}-{i}"), _ => String::new() },
                oninput: move |e| {
                    value.set(e.value());
                    open.set(true);
                    active.set(None);
                },
                onblur: move |_| open.set(false),
                onkeydown: move |e: KeyboardEvent| {
                    if let (_, Some(v)) = on_key(&e, &keys, open, active) {
                        value.set(v);
                    }
                },
            }
            if shown {
                SuggestList {
                    list_id: list_id.clone(),
                    items,
                    active: active(),
                    onpick: move |v| {
                        value.set(v);
                        open.set(false);
                        active.set(None);
                    },
                }
            }
        }
    }
}
