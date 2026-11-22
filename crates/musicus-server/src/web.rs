//! The web client: from a folder (`--web-dir`) or from the copy built into
//! the program (cargo feature `embed-web`, R-OPS-030, a-embedded-web).
//!
//! Both serve the files of the built client and answer every other path
//! outside `/api` with `index.html` (client-side routes of the single-page
//! app). Client files are app code, not sheets, so `MUSICUS_CLIENT_CACHE`
//! does not apply to them.

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, HeaderValue, StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::get,
};
use std::path::PathBuf;
use tower_http::services::{ServeDir, ServeFile};

/// One file of the built-in client. `br`/`gzip` are pre-compressed copies,
/// taken from `<name>.br`/`<name>.gz` next to the file when the build had them.
#[derive(Debug)]
pub struct Asset {
    /// Path below the client folder, with `/`, without a leading slash.
    pub path: &'static str,
    pub body: &'static [u8],
    pub br: Option<&'static [u8]>,
    pub gzip: Option<&'static [u8]>,
    /// Content hash, the same for every encoding (sent as a weak ETag).
    pub hash: &'static str,
}

#[cfg(feature = "embed-web")]
mod embedded {
    // Written by build.rs from the folder in MUSICUS_WEB_DIST: `static ASSETS`, sorted by path.
    include!(concat!(env!("OUT_DIR"), "/web.rs"));
}

/// The client built into this program; `None` without the `embed-web` feature.
pub fn builtin() -> Option<&'static [Asset]> {
    #[cfg(feature = "embed-web")]
    return Some(embedded::ASSETS);
    #[cfg(not(feature = "embed-web"))]
    None
}

/// Which web client the server offers.
#[derive(Debug)]
pub enum WebClient {
    /// API only.
    None,
    /// A folder with the built client (`--web-dir`, development).
    Dir(PathBuf),
    /// The copy inside the program.
    Builtin(&'static [Asset]),
}

impl WebClient {
    /// `--web-dir` wins over the built-in client.
    pub fn choose(web_dir: Option<PathBuf>) -> Self {
        match (web_dir, builtin()) {
            (Some(dir), _) => WebClient::Dir(dir),
            (None, Some(assets)) => WebClient::Builtin(assets),
            (None, None) => WebClient::None,
        }
    }

    /// Adds the client as fallback of `router` (everything outside `/api`).
    pub fn mount<S: Clone + Send + Sync + 'static>(self, router: Router<S>) -> Router<S> {
        match self {
            WebClient::None => router,
            WebClient::Dir(dir) => {
                // Serves `.br`/`.gz` copies next to the files if they exist (dx 0.7 writes none).
                let spa = ServeDir::new(&dir)
                    .precompressed_br()
                    .precompressed_gzip()
                    .fallback(ServeFile::new(dir.join("index.html")));
                router.fallback_service(spa)
            }
            // GET and HEAD; other methods get 405 like from ServeDir.
            WebClient::Builtin(assets) => {
                router.fallback_service(get(move |uri: Uri, headers: HeaderMap| async move {
                    serve_builtin(assets, &uri, &headers)
                }))
            }
        }
    }
}

/// Answers a request for a client file from `assets` (sorted by path).
pub fn serve_builtin(assets: &'static [Asset], uri: &Uri, headers: &HeaderMap) -> Response {
    let mut path = uri.path().trim_start_matches('/').to_owned();
    if path.is_empty() || path.ends_with('/') {
        path.push_str("index.html");
    }
    let find = |p: &str| assets.binary_search_by(|a| a.path.cmp(p)).ok().map(|i| &assets[i]);
    // Unknown paths are client-side routes of the single-page app.
    let Some(asset) = find(&path).or_else(|| find("index.html")) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let etag = format!("W/\"{}\"", asset.hash);
    let mut response = if if_none_match(headers, asset.hash) {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        let accepted = |coding: &str| accepts(headers, coding);
        let (body, encoding) = match (asset.br, asset.gzip) {
            (Some(br), _) if accepted("br") => (br, Some("br")),
            (_, Some(gz)) if accepted("gzip") => (gz, Some("gzip")),
            _ => (asset.body, None),
        };
        let mut r = Response::new(Body::from(body));
        let h = r.headers_mut();
        h.insert(header::CONTENT_TYPE, content_type(asset.path));
        if let Some(encoding) = encoding {
            h.insert(header::CONTENT_ENCODING, HeaderValue::from_static(encoding));
        }
        r
    };
    let h = response.headers_mut();
    h.insert(header::ETAG, HeaderValue::from_str(&etag).expect("hex hash is a header value"));
    // Always revalidated (cheap with the ETag), so a new release is used at once.
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    if asset.br.is_some() || asset.gzip.is_some() {
        h.insert(header::VARY, HeaderValue::from_static("accept-encoding"));
    }
    response
}

/// MIME type by extension (`nosniff` is set, so it must be right). JavaScript
/// is always `text/javascript` (RFC 9239), also for pdf.js's `.mjs` modules.
fn content_type(path: &str) -> HeaderValue {
    if path.ends_with(".mjs") {
        return HeaderValue::from_static("text/javascript");
    }
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    HeaderValue::from_str(mime.as_ref()).expect("MIME type is a header value")
}

/// `If-None-Match` names this content (weak comparison) or is `*`.
fn if_none_match(headers: &HeaderMap, hash: &str) -> bool {
    headers.get_all(header::IF_NONE_MATCH).iter().filter_map(|v| v.to_str().ok()).flat_map(|v| v.split(',')).any(
        |tag| {
            let tag = tag.trim();
            tag == "*" || tag.trim_start_matches("W/").trim_matches('"') == hash
        },
    )
}

/// `Accept-Encoding` lists `coding` with a quality above 0.
fn accepts(headers: &HeaderMap, coding: &str) -> bool {
    headers.get_all(header::ACCEPT_ENCODING).iter().filter_map(|v| v.to_str().ok()).flat_map(|v| v.split(',')).any(
        |item| {
            let mut parts = item.split(';');
            let name = parts.next().unwrap_or("").trim();
            let q = parts
                .filter_map(|p| p.trim().strip_prefix("q="))
                .next()
                .map_or(1.0, |q| q.trim().parse::<f32>().unwrap_or(0.0));
            name.eq_ignore_ascii_case(coding) && q > 0.0
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    static ASSETS: &[Asset] = &[
        Asset { path: "assets/app.js", body: b"js", br: Some(b"js-br"), gzip: Some(b"js-gz"), hash: "a1" },
        Asset { path: "assets/app.wasm", body: b"\0asm", br: None, gzip: None, hash: "a2" },
        Asset { path: "index.html", body: b"<html>", br: None, gzip: None, hash: "a3" },
        Asset { path: "vendor/pdfjs/pdf.min.mjs", body: b"mjs", br: None, gzip: None, hash: "a4" },
    ];

    fn get(path: &str, headers: &[(&'static str, &'static str)]) -> Response {
        let mut map = HeaderMap::new();
        for (k, v) in headers {
            map.append(*k, HeaderValue::from_static(v));
        }
        serve_builtin(ASSETS, &path.parse().unwrap(), &map)
    }

    fn header<'a>(r: &'a Response, name: &str) -> Option<&'a str> {
        r.headers().get(name).map(|v| v.to_str().unwrap())
    }

    #[test]
    fn serves_files_with_their_mime_type() {
        for (path, mime) in [
            ("/", "text/html"),
            ("/index.html", "text/html"),
            ("/assets/app.wasm", "application/wasm"),
            ("/assets/app.js", "text/javascript"),
            ("/vendor/pdfjs/pdf.min.mjs", "text/javascript"),
        ] {
            let r = get(path, &[]);
            assert_eq!(r.status(), StatusCode::OK, "{path}");
            assert_eq!(header(&r, "content-type"), Some(mime), "{path}");
            assert_eq!(header(&r, "cache-control"), Some("no-cache"));
        }
    }

    #[test]
    fn unknown_paths_get_the_app() {
        for path in ["/library", "/parts/12/read?page=3", "/missing.js", "/a/b/"] {
            let r = get(path, &[]);
            assert_eq!(r.status(), StatusCode::OK);
            assert_eq!(header(&r, "content-type"), Some("text/html"));
            assert_eq!(header(&r, "etag"), Some("W/\"a3\""));
        }
    }

    #[test]
    fn picks_a_precompressed_copy() {
        let r = get("/assets/app.js", &[("accept-encoding", "gzip, deflate, br")]);
        assert_eq!(header(&r, "content-encoding"), Some("br"));
        assert_eq!(header(&r, "vary"), Some("accept-encoding"));
        assert_eq!(header(&r, "content-type"), Some("text/javascript"));
        let r = get("/assets/app.js", &[("accept-encoding", "gzip, br;q=0")]);
        assert_eq!(header(&r, "content-encoding"), Some("gzip"));
        let r = get("/assets/app.js", &[("accept-encoding", "identity")]);
        assert_eq!(header(&r, "content-encoding"), None);
        let r = get("/assets/app.wasm", &[("accept-encoding", "br")]);
        assert_eq!(header(&r, "content-encoding"), None);
        assert_eq!(header(&r, "vary"), None);
    }

    #[test]
    fn revalidation_with_etag() {
        let r = get("/assets/app.js", &[("if-none-match", "W/\"a1\"")]);
        assert_eq!(r.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(header(&r, "etag"), Some("W/\"a1\""));
        let r = get("/assets/app.js", &[("if-none-match", "\"zz\", \"a1\"")]);
        assert_eq!(r.status(), StatusCode::NOT_MODIFIED);
        let r = get("/assets/app.js", &[("if-none-match", "W/\"old\"")]);
        assert_eq!(r.status(), StatusCode::OK);
    }

    #[test]
    fn builtin_table_is_sorted_by_path() {
        // build.rs sorts; the lookup relies on it.
        for assets in [Some(ASSETS), builtin()].into_iter().flatten() {
            assert!(assets.windows(2).all(|w| w[0].path < w[1].path));
        }
    }
}
