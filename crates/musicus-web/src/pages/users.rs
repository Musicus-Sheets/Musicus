//! User accounts (admins only): create with a one-time invite link, change
//! roles, disable and enable, issue new invite links.

use crate::{
    api, bridge,
    ui::{ErrorBox, Loading, NoAccess, is_admin},
};
use dioxus::prelude::*;
use musicus_api::{CreateUser, CreatedUser, Invite, Role, UpdateUser, UserSummary};
use musicus_i18n::t;

fn invite_link(invite: &Invite) -> String {
    let origin = web_sys::window().and_then(|w| w.location().origin().ok()).unwrap_or_default();
    format!("{origin}/invite/{}", invite.token)
}

#[component]
pub fn Users() -> Element {
    let admin = is_admin();
    let mut users = use_resource(|| api::get::<Vec<UserSummary>>("/api/users"));
    let mut username = use_signal(String::new);
    let mut display = use_signal(String::new);
    let mut role = use_signal(|| Role::Member);
    let mut link = use_signal(|| None::<(String, String)>);
    let mut error = use_signal(|| None::<String>);

    let create = move |e: FormEvent| async move {
        e.prevent_default();
        let input = CreateUser { username: username(), display_name: display(), role: role() };
        match api::post::<_, CreatedUser>("/api/users", &input).await {
            Ok(c) => {
                link.set(Some((c.user.display_name.clone(), invite_link(&c.invite))));
                username.set(String::new());
                display.set(String::new());
                users.restart();
            }
            Err(e) => error.set(Some(e.message)),
        }
    };
    if !admin {
        return rsx! { NoAccess {} };
    }
    rsx! {
        main {
            div { class: "page-head", h1 { {t::admin_users_title()} } }
            if let Some(e) = error() { ErrorBox { message: e } }
            if let Some((name, url)) = link() {
                div { class: "card highlight",
                    p { crate::ui::WithStrong { text: t::admin_users_invite_text(crate::ui::MARK), strong: name.clone() } }
                    div { class: "inline",
                        input { class: "grow", readonly: true, value: "{url}" }
                        button { onclick: move |_| { let url = url.clone(); async move { bridge::copy_text(&url).await; } }, {t::admin_users_copy()} }
                    }
                }
            }
            section { class: "card",
                h2 { {t::admin_users_new()} }
                form { class: "inline", onsubmit: create,
                    input { placeholder: t::login_username(), required: true, value: "{username}", oninput: move |e| username.set(e.value()) }
                    input { placeholder: t::admin_users_display_name(), required: true, value: "{display}", oninput: move |e| display.set(e.value()) }
                    select { onchange: move |e| role.set(Role::parse(&e.value()).unwrap_or(Role::Member)),
                        option { value: "member", selected: role() == Role::Member, {t::common_role_member()} }
                        option { value: "librarian", selected: role() == Role::Librarian, {t::common_role_librarian()} }
                        option { value: "admin", selected: role() == Role::Admin, {t::common_role_admin()} }
                    }
                    button { class: "primary", r#type: "submit", {t::admin_users_create()} }
                }
            }
            section { class: "card",
                h2 { {t::admin_users_title()} }
                match &*users.read() {
                    None => rsx! { Loading {} },
                    Some(Err(e)) => rsx! { ErrorBox { message: e.message.clone() } },
                    Some(Ok(list)) => rsx! {
                        table { class: "table",
                            thead { tr { th { {t::admin_users_col_name()} } th { {t::login_username()} } th { {t::admin_users_col_role()} } th { {t::admin_users_col_status()} } th {} } }
                            tbody {
                                for u in list.iter().cloned() {
                                    UserRow { key: "{u.id}", user: u,
                                        onchange: move |_| users.restart(),
                                        oninvite: move |l: (String, String)| link.set(Some(l)),
                                        onerror: move |m: String| error.set(Some(m)) }
                                }
                            }
                        }
                    },
                }
            }
        }
    }
}

#[component]
fn UserRow(
    user: UserSummary,
    onchange: EventHandler<()>,
    oninvite: EventHandler<(String, String)>,
    onerror: EventHandler<String>,
) -> Element {
    let id = user.id;
    let name = user.display_name.clone();
    let update = move |body: UpdateUser| async move {
        match api::patch::<_, UserSummary>(&format!("/api/users/{id}"), &body).await {
            Ok(_) => onchange.call(()),
            Err(e) => onerror.call(e.message),
        }
    };
    let invite = move |_| {
        let name = name.clone();
        async move {
            match api::post::<_, Invite>(&format!("/api/users/{id}/invite"), &serde_json::json!({})).await {
                Ok(i) => oninvite.call((name, invite_link(&i))),
                Err(e) => onerror.call(e.message),
            }
        }
    };
    let disabled = user.disabled;
    rsx! {
        tr { class: if disabled { "disabled" },
            td { "{user.display_name}" }
            td { "{user.username}" }
            td {
                select {
                    onchange: move |e| update(UpdateUser { role: Role::parse(&e.value()), ..Default::default() }),
                    option { value: "member", selected: user.role == Role::Member, {t::common_role_member()} }
                    option { value: "librarian", selected: user.role == Role::Librarian, {t::common_role_librarian()} }
                    option { value: "admin", selected: user.role == Role::Admin, {t::common_role_admin()} }
                }
            }
            td {
                if disabled { {t::admin_users_disabled()} } else if user.has_password { {t::admin_users_active()} } else { span { class: "muted", {t::admin_users_invited()} } }
            }
            td { class: "actions",
                if !disabled {
                    button { onclick: invite, if user.has_password { {t::admin_users_reset_link()} } else { {t::admin_users_new_invite()} } }
                }
                button {
                    onclick: move |_| update(UpdateUser { disabled: Some(!disabled), ..Default::default() }),
                    if disabled { {t::admin_users_enable()} } else { {t::admin_users_disable()} }
                }
            }
        }
    }
}
