//! Layout and small shared components.

use crate::{Route, Session, api, session};
use dioxus::prelude::*;
use gloo_net::http::Method;
use musicus_api::Role;
use musicus_i18n::t;

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

/// Frame of all pages except the reader: navigation bar plus content
/// (Library, Programmes, Instruments; Groups and Import for librarians; Users for admins). Sends
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
        return rsx! { div { class: "loading-page", {t::common_loading()} } };
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
                "aria-label": t::common_nav_menu(),
                onclick: move |_| menu_open.toggle(),
                "☰"
            }
            nav { class: if menu_open() { "open" } else { "" }, onclick: move |_| menu_open.set(false),
                Link { to: Route::Library {}, active_class: "active", {t::common_nav_library()} }
                Link { to: Route::Programmes {}, active_class: "active", {t::common_nav_programmes()} }
                Link { to: Route::Instruments {}, active_class: "active", {t::common_nav_instruments()} }
                if user.role.can_manage_library() {
                    Link { to: Route::Groups {}, active_class: "active", {t::common_nav_groups()} }
                    Link { to: Route::Imports {}, active_class: "active", {t::common_nav_import()} }
                }
                if user.role.is_admin() {
                    Link { to: Route::Users {}, active_class: "active", {t::common_nav_users()} }
                }
                Link { to: Route::Account {}, active_class: "active", "{user.display_name}" }
                button { class: "link", onclick: logout, {t::common_nav_log_out()} }
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
            h1 { {t::common_no_access_title()} }
            p { class: "muted", {t::common_no_access_text()} }
            Link { to: Route::Library {}, {t::common_back_to_library()} }
        }
    }
}

/// Stands for an argument shown in bold: `WithStrong` splits the formatted
/// message here.
pub const MARK: &str = "\u{E000}";

/// A message with one argument in bold: `text` is the message formatted with
/// [`MARK`] in place of that argument, `strong` the argument.
#[component]
pub fn WithStrong(text: String, strong: String) -> Element {
    let (before, after) = text.split_once(MARK).unwrap_or((text.as_str(), ""));
    rsx! { "{before}" strong { "{strong}" } "{after}" }
}

#[component]
pub fn Loading() -> Element {
    rsx! { p { class: "muted", {t::common_loading()} } }
}

pub use crate::locale::{format_date, format_size, format_time};

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
