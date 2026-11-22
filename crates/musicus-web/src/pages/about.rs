//! "About Musicus": the running release, the licence and the source code
//! (R-OPS-034, a-version-public). Public: reachable with and without a login,
//! linked from the login page and the account page.

use crate::{
    Route, api, session,
    ui::{ErrorBox, Loading},
};
use dioxus::prelude::*;
use musicus_api::{LICENSE, SOURCE_URL};
use musicus_i18n::t;

/// The licence text (AGPL-3.0-or-later) on the GNU website.
const LICENSE_URL: &str = "https://www.gnu.org/licenses/agpl-3.0.html";

/// The repository URL as shown: without the scheme.
fn display_url(url: &str) -> &str {
    url.trim_start_matches("https://").trim_start_matches("http://").trim_end_matches('/')
}

#[component]
pub fn About() -> Element {
    let s = session();
    let version = use_resource(api::version);
    let logged_in = s.user.read().is_some();
    rsx! {
        main { class: "auth",
            section { class: "card about",
                h1 { {t::about_title()} }
                dl { class: "about-facts",
                    dt { {t::about_version_label()} }
                    dd { id: "about-version",
                        match &*version.read() {
                            None => rsx! { Loading {} },
                            Some(Ok(v)) => rsx! { "{v}" },
                            Some(Err(e)) => rsx! { ErrorBox { message: e.message.clone() } },
                        }
                    }
                    dt { {t::about_licence_label()} }
                    dd {
                        a { href: LICENSE_URL, rel: "license", {t::about_licence_name()} }
                        " ({LICENSE})"
                    }
                    dt { {t::about_source_label()} }
                    dd {
                        a { href: SOURCE_URL, "{display_url(SOURCE_URL)}" }
                    }
                }
                p { class: "muted small", {t::about_free_software()} }
                if logged_in {
                    Link { to: Route::Library {}, {t::common_back_to_library()} }
                } else if (s.checked)() {
                    Link { to: Route::Login {}, {t::about_go_to_login()} }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::display_url;

    #[test]
    fn source_url_is_shown_without_scheme() {
        assert_eq!(display_url("https://github.com/Musicus-Sheets/Musicus"), "github.com/Musicus-Sheets/Musicus");
        assert_eq!(display_url("http://example.org/musicus/"), "example.org/musicus");
    }
}
