//! The HTTP API under `/api`, plus the web client (`crate::web`).

pub mod annotations;
pub mod auth;
pub mod files;
pub mod groups;
pub mod import;
pub mod library;
pub mod programmes;
pub mod users;

use crate::{error::AppError, state::AppState, web::WebClient};
use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::{HeaderName, HeaderValue, header},
    routing::{get, post, put},
};
use tower_http::{set_header::SetResponseHeaderLayer, trace::TraceLayer};

/// Liveness for load balancers and container health checks: the process
/// answers and the database is readable. Needs no login.
async fn health(axum::extract::State(app): axum::extract::State<AppState>) -> crate::error::ApiResult<&'static str> {
    sqlx::query("SELECT 1").execute(&app.db.read).await?;
    Ok("ok")
}

/// The release that runs (R-OPS-033). Needs no login (a-version-public);
/// never cached, so an open client notices an update at once.
async fn version() -> impl axum::response::IntoResponse {
    ([(header::CACHE_CONTROL, "no-store")], axum::Json(musicus_api::VersionInfo::current()))
}

/// Body limit of an import file list (20 000 paths with hashes).
const IMPORT_LIST_BYTES: usize = 16 * 1024 * 1024;

pub fn api() -> Router<AppState> {
    Router::new()
        // Public: liveness and the running release
        .route("/health", get(health))
        .route("/version", get(version))
        // Authentication and the user's own account
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/me", get(auth::me))
        .route("/auth/password", post(auth::change_password))
        .route("/auth/reader-settings", put(auth::set_reader_settings))
        .route("/auth/sessions", get(auth::list_sessions))
        .route("/auth/sessions/{id}", axum::routing::delete(auth::revoke_session))
        .route("/auth/invites/{token}", get(auth::invite_info).post(auth::accept_invite))
        // Users (admin)
        .route("/users", get(users::list).post(users::create))
        .route("/users/{id}", axum::routing::patch(users::update))
        .route("/users/{id}/invite", post(users::reinvite))
        // Instruments and groups
        .route("/instruments", get(groups::list_instruments).post(groups::create_instrument))
        .route("/instruments/{id}", put(groups::update_instrument).delete(groups::delete_instrument))
        .route("/groups", get(groups::list_groups).post(groups::create_group))
        .route("/groups/{id}", put(groups::rename_group).delete(groups::delete_group))
        .route("/groups/{id}/members", put(groups::set_members))
        .route("/groups/{id}/instrument-grants", put(groups::set_instrument_grants))
        .route("/groups/{id}/score-grants", put(groups::set_score_grants))
        // Library
        .route("/scores", get(library::list_scores).post(library::create_score))
        .route("/tags", get(library::list_tags))
        .route("/scores/{id}", get(library::get_score).put(library::update_score).delete(library::delete_score))
        .route("/scores/{id}/parts", post(library::create_part))
        .route("/parts/{id}", get(library::get_part).put(library::update_part).delete(library::delete_part))
        .route("/parts/{id}/file", put(library::upload_file).layer(DefaultBodyLimit::disable()))
        .route("/parts/{id}/versions", get(library::list_versions))
        .route("/files/{sha256}", get(files::get_file))
        .route("/files/{sha256}/thumbnail", get(files::get_thumbnail))
        .route("/files/{sha256}/layout", get(files::get_layout))
        .route("/files/{sha256}/layout/pages/{page}", put(files::put_correction).delete(files::delete_correction))
        // Mass import (librarians)
        .route("/import/batches", get(import::list_batches).post(import::create_batch))
        .route("/import/batches/{id}", get(import::get_review).put(import::update_batch).delete(import::delete_batch))
        .route(
            "/import/batches/{id}/files",
            post(import::register_files).layer(DefaultBodyLimit::max(IMPORT_LIST_BYTES)),
        )
        .route("/import/batches/{id}/commit", post(import::commit))
        .route("/import/batches/{id}/undo", post(import::undo))
        .route("/import/items", axum::routing::patch(import::update_items))
        .route("/import/files/{id}", get(import::get_staged_file))
        .route("/import/files/{id}/content", put(import::upload_content).layer(DefaultBodyLimit::disable()))
        .route("/import/files/{id}/thumbnail", get(import::get_staged_thumbnail))
        .route("/import/aliases", get(import::list_aliases).put(import::put_alias).delete(import::delete_alias))
        // Programmes
        .route("/programmes", get(programmes::list).post(programmes::create))
        .route("/programmes/{id}", get(programmes::get).put(programmes::update).delete(programmes::delete))
        .route("/programmes/{id}/items", put(programmes::set_items))
        // Annotations
        .route("/parts/{id}/annotation-sets", get(annotations::list).post(annotations::create))
        .route("/annotation-sets/{id}", axum::routing::patch(annotations::update).delete(annotations::delete))
        .route("/annotation-sets/{id}/migrate", post(annotations::migrate))
        .route(
            "/annotation-sets/{id}/pages/{page}",
            get(annotations::get_page)
                .put(annotations::put_page)
                .layer(DefaultBodyLimit::max(musicus_api::ink::MAX_ENCODED_BYTES)),
        )
        .fallback(|| async { AppError::NotFound })
}

/// The whole application: API, web client (if any, `web`), common headers.
pub fn app(state: AppState, web: WebClient) -> Router {
    web.mount(Router::new().nest("/api", api()))
        .layer(SetResponseHeaderLayer::overriding(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")))
        .layer(SetResponseHeaderLayer::overriding(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY")))
        .layer(SetResponseHeaderLayer::overriding(header::REFERRER_POLICY, HeaderValue::from_static("same-origin")))
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
