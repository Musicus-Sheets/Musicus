//! Assignment picker: the list of assigned entries (chips with ×) plus a
//! dropdown with a search field in which several entries are ticked at
//! once. Closing the dropdown (tap outside, Escape, the "Add" button)
//! applies the ticked entries (a-assign-picker). Used for group members and
//! grants, the instruments of a part and the pieces of a programme.

use dioxus::html::Key;
use dioxus::prelude::*;
use musicus_i18n::t;
use std::cell::Cell;
use std::rc::Rc;

/// One entry that can be assigned.
#[derive(Clone, PartialEq, Debug)]
pub struct Choice {
    pub id: i64,
    pub label: String,
    /// Secondary text shown muted next to the label (family, composer, username).
    pub detail: String,
    /// Folded label + detail + extra search text.
    haystack: String,
}

impl Choice {
    /// `extra` is searched but not shown (e.g. arranger, subtitle).
    pub fn new(id: i64, label: impl Into<String>, detail: impl Into<String>, extra: &str) -> Self {
        let label = label.into();
        let detail = detail.into();
        let haystack = fold(&format!("{label} {detail} {extra}"));
        Self { id, label, detail, haystack }
    }

    /// Every whitespace separated term of the (folded) query occurs in the entry.
    fn matches(&self, terms: &[String]) -> bool {
        terms.iter().all(|t| self.haystack.contains(t.as_str()))
    }
}

/// Lower case without diacritics ("Fučík" → "fucik"), like the server's search.
pub fn fold(s: &str) -> String {
    let decomposed = String::from(js_sys::JsString::from(s).normalize("NFD"));
    let mut out = String::with_capacity(decomposed.len());
    for c in decomposed.chars() {
        match c {
            '\u{300}'..='\u{36f}' => {}
            'ß' => out.push_str("ss"),
            'æ' | 'Æ' => out.push_str("ae"),
            'œ' | 'Œ' => out.push_str("oe"),
            'ø' | 'Ø' => out.push('o'),
            'ł' | 'Ł' => out.push('l'),
            'đ' | 'Đ' => out.push('d'),
            c => out.extend(c.to_lowercase()),
        }
    }
    out
}

/// Chips and options in the UI language's order (a-browser-sort).
fn sorted(mut options: Vec<Choice>) -> Vec<Choice> {
    options.sort_by(|a, b| crate::locale::compare(&a.label, &b.label).then(a.id.cmp(&b.id)));
    options
}

thread_local! {
    static NEXT_ID: Cell<u32> = const { Cell::new(0) };
}

pub(crate) fn unique_id() -> u32 {
    NEXT_ID.with(|n| {
        let v = n.get() + 1;
        n.set(v);
        v
    })
}

/// Scrolls the page so the whole panel (incl. its button row) is visible,
/// once the current render is on screen. Not needed for the fixed phone sheet.
fn reveal(panel_id: &str) {
    document::eval(&format!(
        "setTimeout(() => {{ const p = document.getElementById('{panel_id}'); \
         if (!p || getComputedStyle(p).position === 'fixed') return; \
         const over = p.getBoundingClientRect().bottom - window.innerHeight + 12; \
         if (over > 0) window.scrollBy(0, over); }}, 30)"
    ));
}

fn focus_element(id: &str) {
    document::eval(&format!("document.getElementById('{id}')?.focus()")); // i18n-ok: JavaScript
}

/// Assigned entries as chips with a remove button, followed by the picker
/// for the entries not assigned yet. Persisting is up to the caller:
/// `onadd` gets the newly chosen ids when the dropdown closes, `onremove`
/// the id whose × was pressed.
#[component]
pub fn AssignList(
    /// Every entry that exists.
    options: Vec<Choice>,
    assigned: Vec<i64>,
    onadd: EventHandler<Vec<i64>>,
    onremove: EventHandler<i64>,
    /// Text of the button that opens the dropdown, e.g. "+ Add members".
    add_label: String,
    /// Shown when nothing is assigned, e.g. "No members yet."
    empty: String,
    /// Placeholder of the search field.
    placeholder: String,
    /// Disables changes, e.g. while a save runs.
    #[props(default)]
    busy: bool,
    /// Result of the last save, shown next to the button.
    #[props(default)]
    status: Option<Result<String, String>>,
) -> Element {
    let all = sorted(options);
    let chips: Vec<Choice> = all.iter().filter(|c| assigned.contains(&c.id)).cloned().collect();
    let rest: Vec<Choice> = all.into_iter().filter(|c| !assigned.contains(&c.id)).collect();
    rsx! {
        div { class: "assign",
            if chips.is_empty() {
                p { class: "muted small assign-empty", "{empty}" }
            } else {
                ul { class: "assigned",
                    for c in chips.iter() {
                        li { key: "{c.id}", class: "chip",
                            span { class: "chip-text",
                                "{c.label}"
                                if !c.detail.is_empty() { span { class: "muted", " {c.detail}" } }
                            }
                            button {
                                r#type: "button",
                                class: "chip-remove",
                                disabled: busy,
                                title: t::common_remove_entry(&c.label),
                                "aria-label": t::common_remove_entry(&c.label),
                                onclick: {
                                    let id = c.id;
                                    move |_| onremove.call(id)
                                },
                                "×"
                            }
                        }
                    }
                }
            }
            div { class: "assign-actions",
                Picker { options: rest, label: add_label, placeholder, disabled: busy, onapply: move |ids| onadd.call(ids) }
                crate::ui::Status { status }
            }
        }
    }
}

/// Button that opens a dropdown with a search field and a multi-selection
/// list. Ticking does not close it; closing applies the ticked entries in
/// the order they were ticked. Keys: ↓/↑ move between search field and
/// entries, Enter or Space ticks (Enter in the search field ticks the first
/// match), Escape closes and applies, typing on an entry continues the search.
#[component]
pub fn Picker(
    /// The entries that can be chosen (already without the assigned ones,
    /// unless the same entry may be chosen twice).
    options: Vec<Choice>,
    label: String,
    placeholder: String,
    onapply: EventHandler<Vec<i64>>,
    #[props(default)] disabled: bool,
    /// Shown in the dropdown when there is nothing to choose.
    #[props(default = t::common_picker_none_left())]
    none_left: String,
) -> Element {
    let uid = use_hook(unique_id);
    let mut open = use_signal(|| false);
    let mut query = use_signal(String::new);
    let mut picked = use_signal(Vec::<i64>::new);
    let mut trigger = use_signal(|| None::<Rc<MountedData>>);
    let search_id = format!("pick-{uid}-search");
    let list_id = format!("pick-{uid}-list");
    let panel_id = format!("pick-{uid}-panel");
    let option_id = move |id: i64| format!("pick-{uid}-o{id}");

    let mut apply = move |refocus: bool| {
        if !open() {
            return;
        }
        let chosen = picked.take();
        open.set(false);
        query.set(String::new());
        if !chosen.is_empty() {
            onapply.call(chosen);
        }
        if refocus && let Some(t) = trigger() {
            spawn(async move {
                let _ = t.set_focus(true).await;
            });
        }
    };
    let mut toggle = move |id: i64| {
        let mut p = picked.write();
        if let Some(pos) = p.iter().position(|x| *x == id) {
            p.remove(pos);
        } else {
            p.push(id);
        }
    };

    let options = sorted(options);
    let terms: Vec<String> = fold(&query()).split_whitespace().map(str::to_owned).collect();
    let filtered: Vec<Choice> = options.iter().filter(|c| c.matches(&terms)).cloned().collect();
    let ids: Vec<i64> = filtered.iter().map(|c| c.id).collect();
    let count = picked.read().len();
    let first = ids.first().copied();

    rsx! {
        div { class: "picker",
            button {
                r#type: "button",
                class: "picker-toggle",
                disabled,
                "aria-haspopup": "listbox",
                "aria-expanded": if open() { "true" } else { "false" },
                onmounted: move |e: MountedEvent| trigger.set(Some(e.data())),
                onclick: move |_| {
                    if open() { apply(false) } else { picked.set(Vec::new()); query.set(String::new()); open.set(true) }
                },
                "{label}"
            }
            if open() {
                // Catches taps and clicks outside the panel: they close it and apply.
                div { class: "picker-backdrop", onclick: move |_| apply(false) }
                div {
                    id: "{panel_id}",
                    class: "picker-panel",
                    role: "dialog",
                    "aria-label": "{label}",
                    onkeydown: move |e: KeyboardEvent| {
                        if e.key() == Key::Escape {
                            e.prevent_default();
                            e.stop_propagation();
                            apply(true);
                        }
                    },
                    input {
                        id: "{search_id}",
                        class: "picker-search",
                        r#type: "search",
                        role: "combobox",
                        autocomplete: "off",
                        "aria-autocomplete": "list",
                        "aria-expanded": "true",
                        "aria-controls": "{list_id}",
                        "aria-label": "{placeholder}",
                        placeholder: "{placeholder}",
                        value: "{query}",
                        oninput: {
                            let panel_id = panel_id.clone();
                            move |e: FormEvent| {
                                query.set(e.value());
                                // A shorter list may have let the page scroll back; show the whole panel again.
                                reveal(&panel_id);
                            }
                        },
                        onmounted: {
                            let panel_id = panel_id.clone();
                            move |e: MountedEvent| {
                                let panel_id = panel_id.clone();
                                async move {
                                    let _ = e.data().set_focus(true).await;
                                    reveal(&panel_id);
                                }
                            }
                        },
                        onkeydown: move |e: KeyboardEvent| match e.key() {
                            Key::ArrowDown => {
                                e.prevent_default();
                                if let Some(id) = first { focus_element(&option_id(id)) }
                            }
                            Key::Enter => {
                                // Never submit the surrounding form.
                                e.prevent_default();
                                if let Some(id) = first { toggle(id) }
                            }
                            _ => {}
                        },
                    }
                    div { id: "{list_id}", class: "picker-options", role: "listbox", "aria-multiselectable": "true",
                        if options.is_empty() {
                            p { class: "muted small picker-none", "{none_left}" }
                        } else if filtered.is_empty() {
                            p { class: "muted small picker-none", {t::common_picker_no_matches()} }
                        }
                        for (i, c) in filtered.iter().enumerate() {
                            {
                                let id = c.id;
                                let on = picked.read().contains(&id);
                                let prev = if i == 0 { None } else { Some(ids[i - 1]) };
                                let next = ids.get(i + 1).copied();
                                let search_id = search_id.clone();
                                rsx! {
                                    div {
                                        key: "{id}",
                                        id: "{option_id(id)}",
                                        class: if on { "picker-option on" } else { "picker-option" },
                                        role: "option",
                                        tabindex: "-1",
                                        "aria-selected": if on { "true" } else { "false" },
                                        onclick: move |_| toggle(id),
                                        onkeydown: move |e: KeyboardEvent| match e.key() {
                                            Key::Enter => { e.prevent_default(); toggle(id) }
                                            Key::Character(s) if s == " " => { e.prevent_default(); toggle(id) }
                                            Key::ArrowDown => {
                                                e.prevent_default();
                                                if let Some(n) = next { focus_element(&option_id(n)) }
                                            }
                                            Key::ArrowUp => {
                                                e.prevent_default();
                                                match prev {
                                                    Some(p) => focus_element(&option_id(p)),
                                                    None => focus_element(&search_id),
                                                }
                                            }
                                            Key::Character(s) if !e.modifiers().ctrl() && !e.modifiers().meta() => {
                                                // Typing on an entry continues the search.
                                                e.prevent_default();
                                                query.write().push_str(&s);
                                                focus_element(&search_id);
                                            }
                                            _ => {}
                                        },
                                        span { class: "picker-box", "aria-hidden": "true", if on { "✓" } }
                                        span { class: "picker-text",
                                            "{c.label}"
                                            if !c.detail.is_empty() { span { class: "muted small", " {c.detail}" } }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    div { class: "picker-foot",
                        span { class: "muted small", role: "status",
                            if count == 0 { {t::common_picker_tick()} } else { {t::common_picker_selected(count)} }
                        }
                        button { r#type: "button", class: "primary", onclick: move |_| apply(true),
                            if count == 0 { {t::common_close()} } else { {t::common_picker_add(count)} }
                        }
                    }
                }
            }
        }
    }
}
