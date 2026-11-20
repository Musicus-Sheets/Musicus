//! Login and invite (first password / password reset) pages.

use crate::{Route, api, session, ui::ErrorBox};
use dioxus::prelude::*;
use musicus_api::{AcceptInvite, InviteInfo, LoginRequest, LoginResponse};
use musicus_i18n::t;

#[component]
pub fn Login() -> Element {
    let s = session();
    let nav = navigator();
    let mut username = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);
    let mut busy = use_signal(|| false);
    use_effect(move || {
        if s.user.read().is_some() {
            nav.replace(Route::Library {});
        }
    });
    let submit = move |e: FormEvent| async move {
        e.prevent_default();
        busy.set(true);
        let req = LoginRequest { username: username(), password: password(), bearer: false };
        match api::post::<_, LoginResponse>("/api/auth/login", &req).await {
            Ok(r) => {
                let mut user = s.user;
                user.set(Some(r.user));
            }
            Err(e) if e.status == 401 => error.set(Some(t::login_wrong_password())),
            Err(e) => error.set(Some(e.message)),
        }
        busy.set(false);
    };
    rsx! {
        main { class: "auth",
            form { class: "card", onsubmit: submit,
                h1 { "𝄞 Musicus" }
                if let Some(e) = error() {
                    ErrorBox { message: e }
                }
                label { {t::login_username()}
                    input { r#type: "text", autocomplete: "username", autofocus: true, required: true,
                        value: "{username}", oninput: move |e| username.set(e.value()) }
                }
                label { {t::login_password()}
                    input { r#type: "password", autocomplete: "current-password", required: true,
                        value: "{password}", oninput: move |e| password.set(e.value()) }
                }
                button { class: "primary", r#type: "submit", disabled: busy(), {t::login_submit()} }
                p { class: "muted small", {t::login_no_account()} }
            }
        }
    }
}

#[component]
pub fn Invite(token: String) -> Element {
    let s = session();
    let nav = navigator();
    let token = use_signal(|| token);
    let info =
        use_resource(move || async move { api::get::<InviteInfo>(&format!("/api/auth/invites/{}", token())).await });
    let mut password = use_signal(String::new);
    let mut repeat = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);
    let submit = move |e: FormEvent| async move {
        e.prevent_default();
        if password() != repeat() {
            error.set(Some(t::login_invite_mismatch()));
            return;
        }
        match api::post::<_, LoginResponse>(
            &format!("/api/auth/invites/{}", token()),
            &AcceptInvite { password: password() },
        )
        .await
        {
            Ok(r) => {
                let mut user = s.user;
                user.set(Some(r.user));
                nav.replace(Route::Library {});
            }
            Err(e) => error.set(Some(e.message)),
        }
    };
    rsx! {
        main { class: "auth",
            match &*info.read() {
                None => rsx! { p { {musicus_i18n::t::common_loading()} } },
                Some(Err(_)) => rsx! {
                    div { class: "card",
                        h1 { {t::login_invite_expired_title()} }
                        p { {t::login_invite_expired_text()} }
                        Link { to: Route::Login {}, {t::login_invite_go_to_login()} }
                    }
                },
                Some(Ok(i)) => rsx! {
                    form { class: "card", onsubmit: submit,
                        h1 { {t::login_invite_welcome(&i.display_name)} }
                        p { crate::ui::WithStrong { text: t::login_invite_choose(crate::ui::MARK), strong: i.username.clone() } }
                        if let Some(e) = error() {
                            ErrorBox { message: e }
                        }
                        label { {t::account_new_password()}
                            input { r#type: "password", autocomplete: "new-password", minlength: 8, required: true,
                                value: "{password}", oninput: move |e| password.set(e.value()) }
                        }
                        label { {t::login_invite_repeat()}
                            input { r#type: "password", autocomplete: "new-password", required: true,
                                value: "{repeat}", oninput: move |e| repeat.set(e.value()) }
                        }
                        button { class: "primary", r#type: "submit", {t::login_invite_submit()} }
                    }
                },
            }
        }
    }
}
