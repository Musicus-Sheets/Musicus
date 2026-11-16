//! Layout and small shared components.

use crate::{Route, Session, api, session};
use dioxus::prelude::*;
use gloo_net::http::Method;
use musicus_api::Role;

/// Role of the logged-in user, if any. Not a hook: may be called anywhere in
/// a component. The UI only uses it to show or hide management controls; the
/// server checks every request itself (d-inline-management).
pub fn current_role() -> Option<Role> {
    try_consume_context::<Session>().and_then(|s| s.user.read().as_ref().map(|u| u.role))
}

/// Librarians and admins see the controls to add and edit music.
pub fn can_manage() -> bool {
    current_role().is_some_and(Role::can_manage_library)
}

pub fn is_admin() -> bool {
    current_role().is_some_and(Role::is_admin)
}

/// Frame of all pages except the reader: navigation bar plus content. Sends
/// visitors without a session to the login page.
#[component]
pub fn Shell() -> Element {
    let s = session();
    let nav = navigator();
    let mut menu_open = use_signal(|| false);
    use_effect(move || {
        if (s.checked)() && s.user.read().is_none() {
            nav.replace(Route::Login {});
        }
    });
    let Some(user) = s.user.read().clone() else {
        return rsx! { div { class: "loading-page", "Loading…" } };
    };
    let logout = move |_| async move {
        let _ = api::call(Method::POST, "/api/auth/logout").await;
        let mut u = s.user;
        u.set(None);
    };
    rsx! {
        header { class: "topbar",
            Link { class: "brand", to: Route::Library {}, "𝄞 Musicus" }
            button {
                class: "menu-toggle",
                "aria-label": "Menu",
                onclick: move |_| menu_open.toggle(),
                "☰"
            }
            nav { class: if menu_open() { "open" } else { "" }, onclick: move |_| menu_open.set(false),
                Link { to: Route::Library {}, active_class: "active", "Library" }
                Link { to: Route::Programmes {}, active_class: "active", "Programmes" }
                Link { to: Route::Instruments {}, active_class: "active", "Instruments" }
                if user.role.can_manage_library() {
                    Link { to: Route::Groups {}, active_class: "active", "Groups" }
                }
                if user.role.is_admin() {
                    Link { to: Route::Users {}, active_class: "active", "Users" }
                }
                Link { to: Route::Account {}, active_class: "active", "{user.display_name}" }
                button { class: "link", onclick: logout, "Log out" }
            }
        }
        Outlet::<Route> {}
    }
}

#[component]
pub fn ErrorBox(message: String) -> Element {
    rsx! { div { class: "error", role: "alert", "{message}" } }
}

/// Result line after saving: green on success, red on failure.
#[component]
pub fn Status(status: Option<Result<String, String>>) -> Element {
    match status {
        Some(Ok(m)) => rsx! { span { class: "ok", role: "status", "{m}" } },
        Some(Err(m)) => rsx! { span { class: "error-text", role: "alert", "{m}" } },
        None => rsx! {},
    }
}

/// Shown instead of a management page to users whose role does not allow it.
#[component]
pub fn NoAccess() -> Element {
    rsx! {
        main { class: "narrow",
            h1 { "Not available" }
            p { class: "muted", "Your role does not include this page." }
            Link { to: Route::Library {}, "Back to the library" }
        }
    }
}

#[component]
pub fn Loading() -> Element {
    rsx! { p { class: "muted", "Loading…" } }
}

/// Unix seconds as a local date and time.
pub fn format_time(secs: i64) -> String {
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(secs as f64 * 1000.0));
    String::from(date.to_locale_string("default", &wasm_bindgen::JsValue::UNDEFINED))
}

/// `2026-12-24` as a localised date.
pub fn format_date(iso: &str) -> String {
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_str(&format!("{iso}T12:00:00")));
    if date.get_time().is_nan() {
        return iso.to_owned();
    }
    String::from(date.to_locale_date_string("default", &wasm_bindgen::JsValue::UNDEFINED))
}

pub fn format_size(bytes: i64) -> String {
    if bytes >= 1_000_000 { format!("{:.1} MB", bytes as f64 / 1e6) } else { format!("{} KB", (bytes + 999) / 1000) }
}

pub fn confirm(message: &str) -> bool {
    web_sys::window().and_then(|w| w.confirm_with_message(message).ok()).unwrap_or(false)
}

pub fn prompt(message: &str, default: &str) -> Option<String> {
    web_sys::window()
        .and_then(|w| w.prompt_with_message_and_default(message, default).ok().flatten())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

/// Trimmed text, `None` when empty.
pub fn none_if_empty(s: String) -> Option<String> {
    let t = s.trim();
    if t.is_empty() { None } else { Some(t.to_owned()) }
}
