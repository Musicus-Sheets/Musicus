//! The web client built into the program (feature `embed-web`, R-OPS-030).
//!
//! Holds for any built client; CI builds it from the fixture:
//! `MUSICUS_WEB_DIST=crates/musicus-server/tests/fixtures/web cargo test -p musicus-server --features embed-web`.
#![cfg(feature = "embed-web")]

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use musicus_server::{
    open_state, routes,
    state::Settings,
    web::{self, WebClient},
};
use tower::ServiceExt;

async fn server() -> (Router, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let state = open_state(dir.path(), 0, Settings::default()).await.unwrap();
    let assets = web::builtin().expect("built with embed-web");
    assert!(matches!(WebClient::choose(None), WebClient::Builtin(_)));
    (routes::app(state, WebClient::Builtin(assets)), dir)
}

async fn get(router: &Router, path: &str, encoding: &str) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let req = Request::get(path).header(header::ACCEPT_ENCODING, encoding).body(Body::empty()).unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    let (parts, body) = res.into_parts();
    (parts.status, parts.headers, body.collect().await.unwrap().to_bytes().to_vec())
}

fn mime_of(path: &str) -> Option<&'static str> {
    let ext = path.rsplit_once('.')?.1;
    Some(match ext {
        "html" => "text/html",
        "js" | "mjs" => "text/javascript",
        "wasm" => "application/wasm",
        "css" => "text/css",
        _ => return None,
    })
}

#[tokio::test]
async fn every_embedded_file_is_served() {
    let (router, _dir) = server().await;
    let assets = web::builtin().unwrap();
    let index = assets.iter().find(|a| a.path == "index.html").expect("index.html embedded");
    for asset in assets {
        // Compressed copies are folded into their file, never served as files of their own.
        for ext in [".br", ".gz"] {
            if let Some(base) = asset.path.strip_suffix(ext) {
                assert!(assets.iter().all(|a| a.path != base), "{} not folded", asset.path);
            }
        }
        let (status, headers, body) = get(&router, &format!("/{}", asset.path), "identity").await;
        assert_eq!(status, StatusCode::OK, "{}", asset.path);
        assert_eq!(body, asset.body, "{}", asset.path);
        assert!(headers.get(header::CONTENT_ENCODING).is_none());
        if let Some(mime) = mime_of(asset.path) {
            assert_eq!(headers[header::CONTENT_TYPE], mime, "{}", asset.path);
        }
        // Security headers of the whole app apply to the client too.
        assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        if let Some(br) = asset.br {
            let (_, headers, body) = get(&router, &format!("/{}", asset.path), "gzip, br").await;
            assert_eq!(headers[header::CONTENT_ENCODING], "br");
            assert_eq!(body, br);
        }
        if let Some(gz) = asset.gzip {
            let (_, headers, body) = get(&router, &format!("/{}", asset.path), "gzip").await;
            assert_eq!(headers[header::CONTENT_ENCODING], "gzip");
            assert_eq!(body, gz);
        }
    }
    // Client-side routes get the app; the API keeps its own 404.
    for path in ["/", "/library", "/parts/7/read"] {
        let (status, headers, body) = get(&router, path, "identity").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CONTENT_TYPE], "text/html");
        assert_eq!(body, index.body);
    }
    let (status, headers, _) = get(&router, "/api/nothing-here", "identity").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(headers[header::CONTENT_TYPE], "application/json");
    let (status, _, body) = get(&router, "/api/health", "identity").await;
    assert_eq!((status, body.as_slice()), (StatusCode::OK, b"ok".as_slice()));
}

#[tokio::test]
async fn web_dir_overrides_the_builtin_client() {
    let web_dir = tempfile::tempdir().unwrap();
    std::fs::write(web_dir.path().join("index.html"), "from the folder").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let state = open_state(dir.path(), 0, Settings::default()).await.unwrap();
    let client = WebClient::choose(Some(web_dir.path().to_owned()));
    assert!(matches!(client, WebClient::Dir(_)));
    let (status, _, body) = get(&routes::app(state, client), "/library", "identity").await;
    assert_eq!((status, body.as_slice()), (StatusCode::OK, b"from the folder".as_slice()));
}
