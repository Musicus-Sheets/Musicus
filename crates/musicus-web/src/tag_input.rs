//! Tag entry as chips (a-tag-input): every finished tag is an atomic chip
//! styled like the library's tags, with a remove button; one text input
//! sits before, between or after the chips and stands for the caret.
//!
//! - A comma, Enter (never submits the form), Tab and leaving the field
//!   turn the typed text into a chip; text is trimmed, empty text ignored,
//!   a tag that exists already (ignoring case) is not added again but its
//!   chip flashes.
//! - Commas are also handled in the input event, for keyboards that send no
//!   usable key event (Android) and for pasting: "a, b, c" pasted at once
//!   gives three chips.
//! - With an empty input, ← / → (and Home / End) move it between the chips;
//!   Backspace at the start of the input removes the chip before it, Delete
//!   at its end the chip after it. Clicking a chip puts the input after it,
//!   clicking the free space of the field puts it at the end.
//! - While typing, known tags that match are suggested (`suggest.rs`); a
//!   taken suggestion becomes a chip.

use crate::picker::unique_id;
use crate::suggest::{self, SuggestList};
use dioxus::html::Key;
use dioxus::prelude::*;
use wasm_bindgen::JsCast;

/// Splits comma separated text into trimmed, non-empty tags.
pub fn split_tags(s: &str) -> Vec<String> {
    s.split(',').map(str::trim).filter(|t| !t.is_empty()).map(str::to_owned).collect()
}

/// Caret of the input: (selection start, selection end, text length).
fn caret(id: &str) -> Option<(u32, u32, u32)> {
    let input: web_sys::HtmlInputElement = web_sys::window()?.document()?.get_element_by_id(id)?.dyn_into().ok()?;
    let len = input.value().encode_utf16().count() as u32;
    Some((input.selection_start().ok()??, input.selection_end().ok()??, len))
}

/// Focuses the input once the current render is on screen (it may have
/// moved to another place between the chips).
fn refocus(id: &str) {
    document::eval(&format!("requestAnimationFrame(() => document.getElementById('{id}')?.focus())"));
}

/// Position of the input among the chips; `END` keeps it after the last
/// chip even when the tags are replaced (e.g. once a loaded piece arrives).
const END: usize = usize::MAX;

#[component]
pub fn TagInput(
    tags: WriteSignal<Vec<String>>,
    /// Id of the element that labels the field.
    labelledby: String,
    #[props(default)] placeholder: String,
    /// Known tags offered while typing (a-tag-suggest).
    #[props(default)]
    suggestions: Vec<String>,
) -> Element {
    let mut tags = tags;
    let uid = use_hook(unique_id);
    let id = format!("tag-entry-{uid}");
    let mut text = use_signal(String::new);
    let mut cursor = use_signal(|| END);
    let mut flash = use_signal(|| None::<String>);
    let mut flash_gen = use_signal(|| 0u32);
    let mut open = use_signal(|| false);
    let mut active = use_signal(|| None::<usize>);
    let list_id = format!("tag-suggest-{uid}");

    let n = tags.read().len();
    let pos = cursor().min(n);
    let id_for_chips = id.clone();

    let mut set_cursor = move |at: usize| {
        let len = tags.peek().len();
        cursor.set(if at >= len { END } else { at });
    };
    // Inserts tags at the input's place; duplicates make the existing chip flash.
    let mut add = move |words: Vec<String>| {
        let mut at = cursor.peek().min(tags.peek().len());
        for w in words.iter().flat_map(|w| split_tags(w)) {
            let existing = tags.peek().iter().find(|t| t.to_lowercase() == w.to_lowercase()).cloned();
            match existing {
                Some(t) => {
                    flash.set(Some(t.to_lowercase()));
                    let mine = *flash_gen.peek() + 1;
                    flash_gen.set(mine);
                    spawn(async move {
                        gloo_timers::future::TimeoutFuture::new(900).await;
                        if flash_gen() == mine {
                            flash.set(None);
                        }
                    });
                }
                None => {
                    tags.write().insert(at, w);
                    at += 1;
                }
            }
        }
        set_cursor(at);
    };
    let mut commit = move || {
        open.set(false);
        active.set(None);
        let t = text.peek().clone();
        if !t.trim().is_empty() {
            add(vec![t]);
        }
        text.set(String::new());
    };
    // Index of a chip by its text: a blur just before a click may have
    // inserted a chip, so indexes captured while rendering can be stale.
    let index_of = move |t: &str| tags.peek().iter().position(|x| x.to_lowercase() == t.to_lowercase());
    let mut remove = move |i: usize| {
        if i >= tags.peek().len() {
            return;
        }
        tags.write().remove(i);
        let c = *cursor.peek();
        if c != END && i < c {
            cursor.set(c - 1);
        }
    };

    let all = tags.read().clone();
    let (before, after) = all.split_at(pos);
    let at_end = pos == n;
    let width = text.read().chars().count().max(1) + 1;
    let flashing = flash();
    let flashes = move |t: &str| flashing.as_deref() == Some(t.to_lowercase().as_str());
    // Chips before and after the input are two keyed lists around a fixed
    // input element: adding or moving chips never moves the focused input.
    let chip = move |t: &String| {
        let (t_click, t_remove) = (t.clone(), t.clone());
        let (id_click, id_remove) = (id_for_chips.clone(), id_for_chips.clone());
        rsx! {
            Chip {
                key: "{t.to_lowercase()}",
                text: t.clone(),
                flash: flashes(t),
                onselect: move |_| {
                    commit();
                    if let Some(i) = index_of(&t_click) {
                        set_cursor(i + 1);
                    }
                    refocus(&id_click);
                },
                onremove: move |_| {
                    if let Some(i) = index_of(&t_remove) {
                        remove(i);
                    }
                    refocus(&id_remove);
                },
            }
        }
    };
    let id_keys = id.clone();
    let offered = suggest::suggestions(&suggestions, &text.read(), &tags.read());
    let shown = open() && !offered.is_empty();
    let offered_keys = offered.clone();
    // Taking a suggestion: it becomes a chip like typed text.
    let mut take = move |v: String| {
        text.set(String::new());
        add(vec![v]);
        open.set(false);
        active.set(None);
    };

    rsx! {
        div {
            class: "tag-input",
            role: "group",
            "aria-labelledby": "{labelledby}",
            // Free space of the field: continue at the end.
            onclick: {
                let id = id.clone();
                move |_| {
                    commit();
                    cursor.set(END);
                    refocus(&id);
                }
            },
            for t in before.iter() { {chip(t)} }
            // The suggestions hang under the input, wherever it is among the chips.
            span { class: if at_end { "tag-entry-wrap end" } else { "tag-entry-wrap" },
                input {
                    id: "{id}",
                    class: if at_end { "tag-entry end" } else { "tag-entry" },
                    style: if at_end { String::new() } else { format!("width: {width}ch") },
                    r#type: "text",
                    autocomplete: "off",
                    enterkeyhint: "enter",
                    "aria-labelledby": "{labelledby}",
                    role: "combobox",
                    "aria-autocomplete": "list",
                    "aria-expanded": if shown { "true" } else { "false" },
                    "aria-controls": "{list_id}",
                    "aria-activedescendant": match active() { Some(i) if shown => format!("{list_id}-{i}"), _ => String::new() },
                    placeholder: if n == 0 { placeholder.clone() } else { String::new() },
                    value: "{text}",
                    onclick: move |e| e.stop_propagation(),
                    onblur: move |_| commit(),
                    oninput: move |e| {
                        open.set(true);
                        active.set(None);
                        let v = e.value();
                        if v.contains(',') {
                            // Several characters at once with a comma: pasted (or a
                            // suggestion of the keyboard) — take all of it.
                            let pasted = v.chars().count() > text.peek().chars().count() + 1;
                            let mut parts: Vec<String> = v.split(',').map(str::to_owned).collect();
                            let rest = if pasted { String::new() } else { parts.pop().unwrap_or_default() };
                            add(parts);
                            text.set(rest.trim_start().to_owned());
                        } else {
                            text.set(v);
                        }
                    },
                    onkeydown: move |e: KeyboardEvent| {
                        match suggest::on_key(&e, &offered_keys, open, active) {
                            (_, Some(v)) => return take(v),
                            (true, None) => return,
                            _ => {}
                        }
                        let empty = text.peek().is_empty();
                        let n = tags.peek().len();
                        let pos = cursor.peek().min(n);
                        match e.key() {
                            Key::Character(c) if c == "," => {
                                e.prevent_default();
                                commit();
                            }
                            Key::Enter => {
                                // Never submits the surrounding form.
                                e.prevent_default();
                                commit();
                            }
                            Key::Tab => commit(),
                            Key::Backspace if pos > 0 => {
                                if caret(&id_keys).is_some_and(|(s, end, _)| s == 0 && end == 0) {
                                    e.prevent_default();
                                    remove(pos - 1);
                                }
                            }
                            Key::Delete if pos < n => {
                                if caret(&id_keys).is_some_and(|(s, end, len)| s == len && end == len) {
                                    e.prevent_default();
                                    remove(pos);
                                }
                            }
                            Key::ArrowLeft if empty && pos > 0 => {
                                e.prevent_default();
                                set_cursor(pos - 1);
                                refocus(&id_keys);
                            }
                            Key::ArrowRight if empty && pos < n => {
                                e.prevent_default();
                                set_cursor(pos + 1);
                                refocus(&id_keys);
                            }
                            Key::Home if empty && pos > 0 => {
                                e.prevent_default();
                                set_cursor(0);
                                refocus(&id_keys);
                            }
                            Key::End if empty && pos < n => {
                                e.prevent_default();
                                set_cursor(END);
                                refocus(&id_keys);
                            }
                            _ => {}
                        }
                    },
                }
                if shown {
                    SuggestList { list_id: list_id.clone(), items: offered, active: active(), onpick: take }
                }
            }
            for t in after.iter() { {chip(t)} }
        }
    }
}

/// One finished tag: not editable; a click selects the place after it.
#[component]
fn Chip(text: String, flash: bool, onselect: EventHandler<()>, onremove: EventHandler<()>) -> Element {
    rsx! {
        span {
            class: if flash { "tag tag-chip flash" } else { "tag tag-chip" },
            onclick: move |e| {
                e.stop_propagation();
                onselect.call(());
            },
            span { class: "tag-text", "{text}" }
            button {
                r#type: "button",
                class: "tag-remove",
                title: "Remove tag {text}",
                "aria-label": "Remove tag {text}",
                onclick: move |e| {
                    e.stop_propagation();
                    onremove.call(());
                },
                "×"
            }
        }
    }
}
