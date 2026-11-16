//! Musicus web client: library, concert programmes, instruments, the sheet
//! reader with private annotations. Librarians and admins manage the music in
//! the same views (extra buttons), admins also groups and users.

mod api;
mod bridge;
mod ink;
mod pages;
mod picker;
mod suggest;
mod tag_input;
mod ui;

use dioxus::prelude::*;
use musicus_api::CurrentUser;
use pages::{
    account::Account,
    groups::Groups,
    instruments::Instruments,
    library::Library,
    login::{Invite, Login},
    programme_edit::{EditProgramme, NewProgramme},
    programmes::{ProgrammeView, Programmes},
    score_edit::{EditPart, EditScore, NewScore},
    users::Users,
    viewer::{ProgrammeViewer, Viewer},
};
use ui::Shell;

const CSS: Asset = asset!("/assets/main.css");

#[derive(Routable, Clone, PartialEq, Debug)]
#[rustfmt::skip]
pub enum Route {
    #[route("/login")]
    Login {},
    #[route("/invite/:token")]
    Invite { token: String },
    #[route("/parts/:id")]
    Viewer { id: i64 },
    #[route("/programmes/:pid/parts/:id")]
    ProgrammeViewer { pid: i64, id: i64 },
    #[layout(Shell)]
        #[route("/")]
        Library {},
        #[route("/scores/new")]
        NewScore {},
        #[route("/scores/:id/edit")]
        EditScore { id: i64 },
        #[route("/parts/:id/edit")]
        EditPart { id: i64 },
        #[route("/programmes")]
        Programmes {},
        #[route("/programmes/new")]
        NewProgramme {},
        #[route("/programmes/:id")]
        ProgrammeView { id: i64 },
        #[route("/programmes/:id/edit")]
        EditProgramme { id: i64 },
        #[route("/instruments")]
        Instruments {},
        #[route("/groups")]
        Groups {},
        #[route("/users")]
        Users {},
        #[route("/account")]
        Account {},
    #[end_layout]
    // Old bookmarks of the former management area.
    #[redirect("/manage", || Route::Library {})]
    #[redirect("/manage/:..segments", |segments: Vec<String>| { let _ = segments; Route::Library {} })]
    #[route("/:..segments")]
    NotFound { segments: Vec<String> },
}

/// Who is logged in. `checked` is false until `/api/auth/me` answered.
#[derive(Clone, Copy)]
pub struct Session {
    pub user: Signal<Option<CurrentUser>>,
    pub checked: Signal<bool>,
}

pub fn session() -> Session {
    use_context::<Session>()
}

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    let mut user = use_signal(|| None::<CurrentUser>);
    let mut checked = use_signal(|| false);
    use_context_provider(|| Session { user, checked });
    use_future(move || async move {
        if let Ok(me) = api::get::<CurrentUser>("/api/auth/me").await {
            user.set(Some(me));
        }
        checked.set(true);
    });
    rsx! {
        document::Title { "Musicus" }
        document::Meta { name: "viewport", content: "width=device-width, initial-scale=1, viewport-fit=cover" }
        document::Meta { name: "theme-color", content: "#1d2433" }
        document::Stylesheet { href: CSS }
        Router::<Route> {}
    }
}

#[component]
fn NotFound(segments: Vec<String>) -> Element {
    rsx! {
        main { class: "narrow",
            h1 { "Page not found" }
            p { "/{segments.join(\"/\")} does not exist." }
            Link { to: Route::Library {}, "Back to the library" }
        }
    }
}
