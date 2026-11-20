//! End-to-end tests of the HTTP API against a temporary data directory.

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use musicus_api::{
    AnnotationSet, ApiError, CommittedImport, CreatedUser, ErrorCode, Field, Group, Id, ImportBatch, ImportReview,
    Instrument, InstrumentAlias, LoginResponse, PageConflict, Part, Programme, ProgrammeSummary, RegisteredImportFiles,
    Score, UndoneImport,
    import::{
        Language, Level, Note, Source,
        pattern::{PRESETS, PatternError, preset},
        review::{ItemState, ReviewPart, ReviewPiece},
    },
    ink::{self, InkObject, PageLayer, Stroke, Tool},
};
use musicus_server::{bootstrap_admin, open_state, routes, state::Settings};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use tower::ServiceExt;

/// The English name the web client composes for a part (the server sends
/// only its components, a-client-part-names).
fn name(p: &Part) -> String {
    musicus_i18n::set_language("en");
    let names: Vec<&str> = p.instruments.iter().map(|i| i.name.as_str()).collect();
    musicus_api::naming::part_name(
        &musicus_i18n::name_words(),
        p.label.as_deref(),
        &names,
        p.transposition.as_deref(),
        &p.voices,
    )
}

/// The parts in the order the client lists them (by name, numbers by value).
fn sorted(parts: &[Part]) -> Vec<Part> {
    let mut v = parts.to_vec();
    v.sort_by(|a, b| musicus_api::naming::natural_cmp(&name(a), &name(b)));
    v
}

fn names(parts: &[Part]) -> Vec<String> {
    sorted(parts).iter().map(name).collect()
}

/// The English name a part of the import review will get.
fn review_name(p: &musicus_api::import::review::ReviewPart) -> String {
    musicus_i18n::set_language("en");
    p.name(&musicus_i18n::name_words())
}

struct Server {
    router: Router,
    state: musicus_server::state::AppState,
    _dir: tempfile::TempDir,
    admin_invite: String,
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

impl Reply {
    fn json<T: DeserializeOwned>(&self) -> T {
        serde_json::from_slice(&self.body).unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&self.body)))
    }

    /// Status and error code of a failed request (a-error-codes).
    fn error(&self) -> (StatusCode, ErrorCode) {
        let e: ApiError = self.json();
        assert_eq!(e.message, e.code.to_string(), "message is the English text of the code");
        (self.status, e.code)
    }
}

enum Auth<'a> {
    None,
    Bearer(&'a str),
    /// Cookie authentication; `true` adds the CSRF header.
    Cookie(&'a str, bool),
}

impl Server {
    async fn start() -> Self {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings { cookie_secure: false, ..Settings::default() };
        let state = open_state(dir.path(), 64 * 1024 * 1024, settings).await.unwrap();
        let admin_invite = bootstrap_admin(&state).await.unwrap().expect("fresh database");
        assert!(bootstrap_admin(&state).await.unwrap().is_none());
        Server { router: routes::app(state.clone(), None), state, _dir: dir, admin_invite }
    }

    async fn send(&self, method: Method, uri: &str, auth: Auth<'_>, headers: &[(&str, &str)], body: Body) -> Reply {
        let mut req = Request::builder().method(method).uri(uri);
        match auth {
            Auth::None => {}
            Auth::Bearer(t) => req = req.header(header::AUTHORIZATION, format!("Bearer {t}")),
            Auth::Cookie(t, csrf) => {
                req = req.header(header::COOKIE, format!("musicus_session={t}"));
                if csrf {
                    req = req.header(musicus_api::CSRF_HEADER, "1");
                }
            }
        }
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let response = self.router.clone().oneshot(req.body(body).unwrap()).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let body = response.into_body().collect().await.unwrap().to_bytes().to_vec();
        Reply { status, headers, body }
    }

    async fn json(&self, method: Method, uri: &str, token: &str, body: impl Serialize) -> Reply {
        self.send(
            method,
            uri,
            Auth::Bearer(token),
            &[("content-type", "application/json")],
            Body::from(serde_json::to_vec(&body).unwrap()),
        )
        .await
    }

    async fn get(&self, uri: &str, token: &str) -> Reply {
        self.send(Method::GET, uri, Auth::Bearer(token), &[], Body::empty()).await
    }

    async fn ok<T: DeserializeOwned>(&self, method: Method, uri: &str, token: &str, body: impl Serialize) -> T {
        let r = self.json(method.clone(), uri, token, body).await;
        assert!(r.status.is_success(), "{method} {uri}: {} {}", r.status, String::from_utf8_lossy(&r.body));
        r.json()
    }

    async fn login(&self, username: &str, password: &str) -> String {
        let r = self
            .send(
                Method::POST,
                "/api/auth/login",
                Auth::None,
                &[("content-type", "application/json")],
                Body::from(json!({"username": username, "password": password, "bearer": true}).to_string()),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{}", String::from_utf8_lossy(&r.body));
        r.json::<LoginResponse>().token.unwrap()
    }

    /// Accepts an invite and returns the session cookie value.
    async fn accept_invite(&self, token: &str, password: &str) -> String {
        let r = self
            .send(
                Method::POST,
                &format!("/api/auth/invites/{token}"),
                Auth::None,
                &[("content-type", "application/json")],
                Body::from(json!({ "password": password }).to_string()),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{}", String::from_utf8_lossy(&r.body));
        let cookie = r.headers[header::SET_COOKIE].to_str().unwrap();
        assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Lax"));
        cookie.split(';').next().unwrap().trim_start_matches("musicus_session=").to_owned()
    }

    /// Number of library hits for a URL-encoded search.
    async fn hits(&self, q: &str, token: &str) -> usize {
        self.get(&format!("/api/scores?q={q}"), token).await.json::<Vec<Score>>().len()
    }

    async fn admin(&self) -> String {
        self.accept_invite(&self.admin_invite, "admin-password-1").await;
        self.login("admin", "admin-password-1").await
    }

    async fn member(&self, admin: &str, username: &str) -> (Id, String) {
        let created: CreatedUser = self
            .ok(
                Method::POST,
                "/api/users",
                admin,
                json!({"username": username, "display_name": username, "role": "member"}),
            )
            .await;
        self.accept_invite(&created.invite.token, "member-password-1").await;
        (created.user.id, self.login(username, "member-password-1").await)
    }

    async fn upload(&self, token: &str, part: Id, bytes: Vec<u8>) -> Reply {
        self.send(Method::PUT, &format!("/api/parts/{part}/file"), Auth::Bearer(token), &[], Body::from(bytes)).await
    }
}

/// A minimal valid PDF with `pages` empty A4 pages; `tag` makes contents unique.
fn tiny_pdf(pages: usize, tag: &str) -> Vec<u8> {
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        format!(
            "<< /Type /Pages /Kids [{}] /Count {pages} >>",
            (0..pages).map(|i| format!("{} 0 R", i + 3)).collect::<Vec<_>>().join(" ")
        ),
    ];
    for _ in 0..pages {
        objects.push("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] >>".to_owned());
    }
    let mut pdf = format!("%PDF-1.7\n%{tag}\n").into_bytes();
    let mut offsets = Vec::new();
    for (i, obj) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend(format!("{} 0 obj\n{obj}\nendobj\n", i + 1).bytes());
    }
    let xref = pdf.len();
    pdf.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
    for off in offsets {
        pdf.extend(format!("{off:010} 00000 n \n").bytes());
    }
    pdf.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).bytes());
    pdf
}

/// A PDF with `pages` A4 pages of `staves` single-staff systems (lines 5 pt
/// apart, a short title line on top), like an engraved part.
fn staff_pdf(pages: usize, staves: usize) -> Vec<u8> {
    let mut content = String::from("0.6 w\n");
    content += "250 790 m 345 790 l 4 w S 0.6 w\n";
    for k in 0..staves {
        let top = 740.0 - k as f64 * 80.0;
        for l in 0..5 {
            let y = top - l as f64 * 5.0;
            content += &format!("40 {y} m 555 {y} l S\n");
        }
        // A barline every quarter of the staff.
        for b in 1..4 {
            let x = 40.0 + b as f64 * 128.75;
            content += &format!("{x} {top} m {x} {} l S\n", top - 20.0);
        }
    }
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        format!(
            "<< /Type /Pages /Kids [{}] /Count {pages} >>",
            (0..pages).map(|i| format!("{} 0 R", i + 4)).collect::<Vec<_>>().join(" ")
        ),
        format!("<< /Length {} >>\nstream\n{content}endstream", content.len()),
    ];
    for _ in 0..pages {
        objects.push("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 3 0 R >>".to_owned());
    }
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, obj) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend(format!("{} 0 obj\n{obj}\nendobj\n", i + 1).bytes());
    }
    let xref = pdf.len();
    pdf.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
    for off in offsets {
        pdf.extend(format!("{off:010} 00000 n \n").bytes());
    }
    pdf.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).bytes());
    pdf
}

fn layer(ids: &[u64]) -> Vec<u8> {
    let objects = ids
        .iter()
        .map(|&id| {
            InkObject::Stroke(Stroke::from_points(
                id,
                Tool::Pen,
                0xff0000ff,
                200,
                &[(100, 100, 128), (5000, 5200, 140)],
            ))
        })
        .collect();
    ink::encode(&PageLayer { objects })
}

/// An orchestra with two instruments, a group "Trumpets" with member anna,
/// one score with a trumpet and a flute part, both with files.
struct Library {
    admin: String,
    anna: String,
    anna_id: Id,
    trumpets: Id,
    score: Id,
    trumpet_part: Part,
    flute_part: Part,
}

async fn library(s: &Server) -> Library {
    let admin = s.admin().await;
    let trumpet: Instrument =
        s.ok(Method::POST, "/api/instruments", &admin, json!({"name": "Trumpet", "family": "Brass"})).await;
    let flute: Instrument = s.ok(Method::POST, "/api/instruments", &admin, json!({"name": "Flute"})).await;
    let group: Group = s.ok(Method::POST, "/api/groups", &admin, json!({"name": "Trumpets"})).await;
    let (anna_id, anna) = s.member(&admin, "anna").await;
    s.ok::<Group>(Method::PUT, &format!("/api/groups/{}/members", group.id), &admin, json!({"ids": [anna_id]})).await;
    let g: Group = s
        .ok(Method::PUT, &format!("/api/groups/{}/instrument-grants", group.id), &admin, json!({"ids": [trumpet.id]}))
        .await;
    assert_eq!(g.member_ids, vec![anna_id]);

    let score: Score = s
        .ok(
            Method::POST,
            "/api/scores",
            &admin,
            json!({"title": "Florentiner Marsch", "composer": "Julius Fučík", "genre": "March", "tags": ["Encore"]}),
        )
        .await;
    let tp: Part = s
        .ok(
            Method::POST,
            &format!("/api/scores/{}/parts", score.id),
            &admin,
            json!({"instrument_ids": [trumpet.id], "voices": ["2"], "transposition": "Bb"}),
        )
        .await;
    let fp: Part = s
        .ok(
            Method::POST,
            &format!("/api/scores/{}/parts", score.id),
            &admin,
            json!({"label": "Solo Flute", "instrument_ids": [flute.id]}),
        )
        .await;
    let tp: Part = s.upload(&admin, tp.id, tiny_pdf(4, "trumpet")).await.json();
    let fp: Part = s.upload(&admin, fp.id, tiny_pdf(2, "flute")).await.json();
    assert_eq!(tp.file.as_ref().unwrap().page_count, Some(4));
    assert_eq!((tp.label.as_deref(), name(&tp).as_str()), (None, "Trumpet in B♭ 2"));
    assert_eq!(name(&fp), "Solo Flute");
    Library { admin, anna, anna_id, trumpets: group.id, score: score.id, trumpet_part: tp, flute_part: fp }
}

#[tokio::test]
async fn login_sessions_and_csrf() {
    let s = Server::start().await;
    let health = s.send(Method::GET, "/api/health", Auth::None, &[], Body::empty()).await;
    assert_eq!((health.status, health.body.as_slice()), (StatusCode::OK, b"ok".as_slice()));
    let cookie = s.accept_invite(&s.admin_invite, "admin-password-1").await;
    // The invite is single-use.
    let again =
        s.send(Method::GET, &format!("/api/auth/invites/{}", s.admin_invite), Auth::None, &[], Body::empty()).await;
    assert_eq!(again.status, StatusCode::NOT_FOUND);

    let me = s.send(Method::GET, "/api/auth/me", Auth::Cookie(&cookie, false), &[], Body::empty()).await;
    assert_eq!(me.json::<Value>()["role"], "admin");
    assert_eq!(
        s.send(Method::GET, "/api/auth/me", Auth::None, &[], Body::empty()).await.status,
        StatusCode::UNAUTHORIZED
    );

    // Cookie-authenticated writes need the CSRF header; bearer tokens do not.
    let body = || Body::from(json!({"name": "Horn"}).to_string());
    let ct = [("content-type", "application/json")];
    assert_eq!(
        s.send(Method::POST, "/api/instruments", Auth::Cookie(&cookie, false), &ct, body()).await.error(),
        (StatusCode::FORBIDDEN, ErrorCode::CsrfMissing)
    );
    assert_eq!(
        s.send(Method::POST, "/api/instruments", Auth::Cookie(&cookie, true), &ct, body()).await.status,
        StatusCode::OK
    );

    let wrong = || Body::from(json!({"username": "ADMIN", "password": "nope-nope", "bearer": true}).to_string());
    for _ in 0..10 {
        assert_eq!(
            s.send(Method::POST, "/api/auth/login", Auth::None, &ct, wrong()).await.status,
            StatusCode::UNAUTHORIZED
        );
    }
    let (status, code) = s.send(Method::POST, "/api/auth/login", Auth::None, &ct, wrong()).await.error();
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert!(matches!(code, ErrorCode::TooManyAttempts { minutes } if minutes > 0), "{code:?}");

    // Logout ends exactly this session.
    let r = s.send(Method::POST, "/api/auth/logout", Auth::Cookie(&cookie, true), &[], Body::empty()).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(
        s.send(Method::GET, "/api/auth/me", Auth::Cookie(&cookie, false), &[], Body::empty()).await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn members_only_reach_granted_parts() {
    let s = Server::start().await;
    let lib = library(&s).await;

    // Anna sees the score, but only the trumpet part.
    let scores: Vec<Score> = s.get("/api/scores", &lib.anna).await.json();
    assert_eq!(scores.len(), 1);
    assert_eq!(scores[0].parts.iter().map(|p| p.id).collect::<Vec<_>>(), vec![lib.trumpet_part.id]);

    let trumpet_sha = &lib.trumpet_part.file.as_ref().unwrap().sha256;
    let flute_sha = &lib.flute_part.file.as_ref().unwrap().sha256;
    let file = s.get(&format!("/api/files/{trumpet_sha}"), &lib.anna).await;
    assert_eq!(file.status, StatusCode::OK);
    assert_eq!(file.headers[header::CONTENT_TYPE], "application/pdf");
    assert_eq!(file.headers[header::CACHE_CONTROL], "private, no-store");
    assert_eq!(file.body, tiny_pdf(4, "trumpet"));
    // Denied files look like missing ones.
    assert_eq!(s.get(&format!("/api/files/{flute_sha}"), &lib.anna).await.status, StatusCode::NOT_FOUND);
    assert_eq!(s.get(&format!("/api/files/{flute_sha}/thumbnail"), &lib.anna).await.status, StatusCode::NOT_FOUND);
    assert_eq!(
        s.get(&format!("/api/files/{trumpet_sha}/thumbnail"), &lib.anna).await.headers[header::CONTENT_TYPE],
        "image/png"
    );
    assert_eq!(
        s.get(&format!("/api/parts/{}/versions", lib.flute_part.id), &lib.anna).await.status,
        StatusCode::NOT_FOUND
    );

    // Range and conditional requests.
    let uri = format!("/api/files/{trumpet_sha}");
    let range = s.send(Method::GET, &uri, Auth::Bearer(&lib.anna), &[("range", "bytes=0-7")], Body::empty()).await;
    assert_eq!(range.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(range.body, b"%PDF-1.7");
    let etag = format!("\"{trumpet_sha}\"");
    let cond = s.send(Method::GET, &uri, Auth::Bearer(&lib.anna), &[("if-none-match", &etag)], Body::empty()).await;
    assert_eq!(cond.status, StatusCode::NOT_MODIFIED);

    // A score grant opens every part of that score.
    s.ok::<Group>(
        Method::PUT,
        &format!("/api/groups/{}/score-grants", lib.trumpets),
        &lib.admin,
        json!({"ids": [lib.score]}),
    )
    .await;
    assert_eq!(s.get(&format!("/api/files/{flute_sha}"), &lib.anna).await.status, StatusCode::OK);

    // Removing her from the group revokes everything on the next request.
    s.ok::<Group>(Method::PUT, &format!("/api/groups/{}/members", lib.trumpets), &lib.admin, json!({"ids": []})).await;
    assert_eq!(s.get(&format!("/api/files/{trumpet_sha}"), &lib.anna).await.status, StatusCode::NOT_FOUND);
    assert!(s.get("/api/scores", &lib.anna).await.json::<Vec<Score>>().is_empty());

    // Members cannot manage the library.
    let r = s.json(Method::POST, "/api/scores", &lib.anna, json!({"title": "x"})).await;
    assert_eq!(r.error(), (StatusCode::FORBIDDEN, ErrorCode::LibrarianOnly));
    assert_eq!(s.get("/api/users", &lib.anna).await.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn uploads_are_versioned_and_validated() {
    let s = Server::start().await;
    let lib = library(&s).await;
    let part = lib.trumpet_part.id;

    assert_eq!(
        s.upload(&lib.admin, part, b"<?xml version='1.0'?>".to_vec()).await.error(),
        (StatusCode::UNSUPPORTED_MEDIA_TYPE, ErrorCode::NotPdf)
    );
    // Same content again: no new version.
    let same: Part = s.upload(&lib.admin, part, tiny_pdf(4, "trumpet")).await.json();
    assert_eq!(same.file.unwrap().version, 1);
    let v2: Part = s.upload(&lib.admin, part, tiny_pdf(4, "trumpet corrected")).await.json();
    assert_eq!(v2.file.as_ref().unwrap().version, 2);
    let versions: Vec<musicus_api::FileVersion> = s.get(&format!("/api/parts/{part}/versions"), &lib.anna).await.json();
    assert_eq!(versions.iter().map(|v| v.version).collect::<Vec<_>>(), vec![2, 1]);
    // Old versions stay readable for annotations drawn on them.
    let old = &lib.trumpet_part.file.unwrap().sha256;
    assert_eq!(s.get(&format!("/api/files/{old}"), &lib.anna).await.status, StatusCode::OK);
}

#[tokio::test]
async fn annotations_are_private_versioned_and_conflict_checked() {
    let s = Server::start().await;
    let lib = library(&s).await;
    let part = lib.trumpet_part.id;
    let set: AnnotationSet = s
        .ok(Method::POST, &format!("/api/parts/{part}/annotation-sets"), &lib.anna, json!({"name": "Christmas 2026"}))
        .await;
    assert!(set.is_default);
    assert_eq!(set.file_version, 1);
    // The reader's own set has no name (shown in the UI language); one per part.
    let sets = format!("/api/parts/{part}/annotation-sets");
    let unnamed: AnnotationSet = s.ok(Method::POST, &sets, &lib.anna, json!({})).await;
    assert_eq!((unnamed.name, unnamed.is_default), (None, false));
    assert_eq!(s.json(Method::POST, &sets, &lib.anna, json!({"name": " "})).await.status, StatusCode::CONFLICT);
    let page = |n: i64| format!("/api/annotation-sets/{}/pages/{n}", set.id);
    let put = |uri: String, rev: &'static str, body: Vec<u8>| {
        let token = lib.anna.clone();
        let s = &s;
        async move { s.send(Method::PUT, &uri, Auth::Bearer(&token), &[("if-match", rev)], Body::from(body)).await }
    };

    let empty = s.get(&page(0), &lib.anna).await;
    assert_eq!((empty.status, empty.headers[header::ETAG].to_str().unwrap()), (StatusCode::NO_CONTENT, "\"0\""));
    assert_eq!(
        s.send(Method::PUT, &page(0), Auth::Bearer(&lib.anna), &[], Body::from(layer(&[1]))).await.error(),
        (StatusCode::PRECONDITION_REQUIRED, ErrorCode::IfMatchRequired)
    );
    let saved = put(page(0), "\"0\"", layer(&[1])).await;
    assert_eq!(saved.status, StatusCode::OK);
    assert_eq!(saved.json::<Value>()["revision"], 1);
    // A second device that still believes the page is new gets a conflict.
    let conflict = put(page(0), "\"0\"", layer(&[2])).await;
    assert_eq!(conflict.status, StatusCode::CONFLICT);
    assert_eq!(conflict.json::<PageConflict>().current_revision, 1);
    assert_eq!(put(page(0), "1", layer(&[1, 2])).await.status, StatusCode::OK);

    let got = s.get(&page(0), &lib.anna).await;
    assert_eq!(got.headers[header::ETAG], "\"2\"");
    assert_eq!(got.body, layer(&[1, 2]));
    assert_eq!(
        put(page(1), "0", vec![1, 0xff, 0xff]).await.error(),
        (StatusCode::UNPROCESSABLE_ENTITY, ErrorCode::InvalidInk { reason: ink::InkError::Malformed })
    );
    // The part has 4 pages.
    assert_eq!(put(page(4), "0", layer(&[1])).await.status, StatusCode::NOT_FOUND);

    // Nobody else sees the set, admins included.
    let (_, bob) = s.member(&lib.admin, "bob").await;
    s.ok::<Group>(
        Method::PUT,
        &format!("/api/groups/{}/members", lib.trumpets),
        &lib.admin,
        json!({"ids": [lib.anna_id]}),
    )
    .await;
    assert_eq!(s.get(&page(0), &lib.admin).await.status, StatusCode::NOT_FOUND);
    assert_eq!(s.get(&page(0), &bob).await.status, StatusCode::NOT_FOUND);
    assert!(
        s.get(&format!("/api/parts/{part}/annotation-sets"), &lib.admin).await.json::<Vec<AnnotationSet>>().is_empty()
    );

    // Duplicate for another concert; make it the default.
    let copy: AnnotationSet = s
        .ok(
            Method::POST,
            &format!("/api/parts/{part}/annotation-sets"),
            &lib.anna,
            json!({"name": "Spring 2027", "copy_from": set.id}),
        )
        .await;
    assert_eq!(copy.pages.len(), 1);
    assert!(!copy.is_default);
    let copy: AnnotationSet =
        s.ok(Method::PATCH, &format!("/api/annotation-sets/{}", copy.id), &lib.anna, json!({"is_default": true})).await;
    assert!(copy.is_default);
    let sets: Vec<AnnotationSet> = s.get(&format!("/api/parts/{part}/annotation-sets"), &lib.anna).await.json();
    assert_eq!(sets.iter().filter(|s| s.is_default).count(), 1);
    let dup = s
        .json(Method::POST, &format!("/api/parts/{part}/annotation-sets"), &lib.anna, json!({"name": "spring 2027 "}))
        .await;
    assert!(dup.status == StatusCode::OK || dup.status == StatusCode::CONFLICT);

    // A corrected upload with the same page count: the set can move along.
    s.upload(&lib.admin, part, tiny_pdf(4, "corrected")).await;
    let moved: AnnotationSet =
        s.ok(Method::POST, &format!("/api/annotation-sets/{}/migrate", set.id), &lib.anna, json!({})).await;
    assert_eq!(moved.file_version, 2);
    s.upload(&lib.admin, part, tiny_pdf(5, "longer")).await;
    let refused = s.json(Method::POST, &format!("/api/annotation-sets/{}/migrate", set.id), &lib.anna, json!({})).await;
    assert_eq!(refused.error(), (StatusCode::CONFLICT, ErrorCode::PageCountChanged));

    // Losing access hides the annotations too.
    s.ok::<Group>(Method::PUT, &format!("/api/groups/{}/members", lib.trumpets), &lib.admin, json!({"ids": []})).await;
    assert_eq!(s.get(&page(0), &lib.anna).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn programmes_and_search() {
    let s = Server::start().await;
    let lib = library(&s).await;
    let other: Score = s.ok(Method::POST, "/api/scores", &lib.admin, json!({"title": "Flute Concerto"})).await;
    let programme: Programme = s
        .ok(Method::POST, "/api/programmes", &lib.admin, json!({"title": "Christmas concert", "date": "2026-12-24"}))
        .await;
    let p: Programme = s
        .ok(
            Method::PUT,
            &format!("/api/programmes/{}/items", programme.id),
            &lib.admin,
            json!({"ids": [lib.score, other.id, lib.score]}),
        )
        .await;
    assert_eq!(p.items.len(), 3);
    s.ok::<Programme>(Method::POST, "/api/programmes", &lib.admin, json!({"title": "Empty"})).await;
    let bad = s.json(Method::POST, "/api/programmes", &lib.admin, json!({"title": "x", "date": "24.12.2026"})).await;
    assert_eq!(bad.error(), (StatusCode::BAD_REQUEST, ErrorCode::DateFormat));

    // Anna sees only what she plays: the march twice (encore), no concerto.
    let list: Vec<ProgrammeSummary> = s.get("/api/programmes", &lib.anna).await.json();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].item_count, 2);
    let mine: Programme = s.get(&format!("/api/programmes/{}", programme.id), &lib.anna).await.json();
    assert_eq!(mine.items.iter().map(|i| i.position).collect::<Vec<_>>(), vec![0, 2]);
    assert_eq!(mine.items[0].score.parts.len(), 1);

    // Search ignores diacritics and matches prefixes.
    let hits: Vec<Score> = s.get("/api/scores?q=fucik%20flor", &lib.anna).await.json();
    assert_eq!(hits.len(), 1);
    assert!(s.get("/api/scores?q=concerto", &lib.anna).await.json::<Vec<Score>>().is_empty());
    assert_eq!(s.get("/api/scores?q=concerto", &lib.admin).await.json::<Vec<Score>>().len(), 1);
    assert_eq!(s.get("/api/scores?tag=encore", &lib.anna).await.json::<Vec<Score>>().len(), 1);
    assert_eq!(s.get("/api/scores?genre=polka", &lib.anna).await.json::<Vec<Score>>().len(), 0);

    // Tag suggestions: librarians get every tag (merged ignoring case),
    // members only the tags of scores they can see.
    s.ok::<Score>(
        Method::PUT,
        &format!("/api/scores/{}", other.id),
        &lib.admin,
        json!({"title": "Flute Concerto", "tags": ["Solo", "encore"]}),
    )
    .await;
    let all: Vec<String> = s.get("/api/tags", &lib.admin).await.json();
    assert_eq!(all.len(), 2);
    assert!(all.iter().any(|t| t == "Solo") && all.iter().any(|t| t.eq_ignore_ascii_case("encore")));
    let annas: Vec<String> = s.get("/api/tags", &lib.anna).await.json();
    assert_eq!(annas, ["Encore"], "the concerto's tag Solo is hidden from anna");
}

#[tokio::test]
async fn user_administration() {
    let s = Server::start().await;
    let admin = s.admin().await;
    let created: CreatedUser = s
        .ok(
            Method::POST,
            "/api/users",
            &admin,
            json!({"username": "Carla", "display_name": "Carla", "role": "librarian"}),
        )
        .await;
    assert_eq!(created.user.username, "carla");
    assert!(!created.user.has_password);
    let dup = s
        .json(Method::POST, "/api/users", &admin, json!({"username": "carla", "display_name": "C", "role": "member"}))
        .await;
    assert_eq!(dup.error(), (StatusCode::CONFLICT, ErrorCode::AlreadyExists));

    s.accept_invite(&created.invite.token, "carla-password").await;
    let carla = s.login("carla", "carla-password").await;
    assert_eq!(s.get("/api/groups", &carla).await.status, StatusCode::OK);
    assert_eq!(
        s.json(Method::POST, "/api/groups", &carla, json!({"name": "x"})).await.error(),
        (StatusCode::FORBIDDEN, ErrorCode::AdminOnly)
    );

    // Disabling logs out everywhere.
    let r = s.json(Method::PATCH, &format!("/api/users/{}", created.user.id), &admin, json!({"disabled": true})).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(s.get("/api/auth/me", &carla).await.status, StatusCode::UNAUTHORIZED);
    let me: Value = s.get("/api/auth/me", &admin).await.json();
    let self_disable =
        s.json(Method::PATCH, &format!("/api/users/{}", me["id"]), &admin, json!({"disabled": true})).await;
    assert_eq!(self_disable.error(), (StatusCode::CONFLICT, ErrorCode::CannotLockOutSelf));

    // Password change keeps the current session and ends the others.
    let other = s.login("admin", "admin-password-1").await;
    let r = s
        .json(
            Method::POST,
            "/api/auth/password",
            &admin,
            json!({"current_password": "admin-password-1", "new_password": "new-password-2"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(s.get("/api/auth/me", &admin).await.status, StatusCode::OK);
    assert_eq!(s.get("/api/auth/me", &other).await.status, StatusCode::UNAUTHORIZED);
    let sessions: Vec<musicus_api::SessionInfo> = s.get("/api/auth/sessions", &admin).await.json();
    assert_eq!(sessions.len(), 1);
    assert!(sessions[0].current);
}

#[tokio::test]
async fn instruments_are_listed_for_everyone_and_kept_while_in_use() {
    let s = Server::start().await;
    let lib = library(&s).await;

    // Every user sees the list with families (the Instruments page).
    let list: Vec<Instrument> = s.get("/api/instruments", &lib.anna).await.json();
    let names: Vec<_> = list.iter().map(|i| (i.name.as_str(), i.family.as_deref())).collect();
    assert_eq!(names, vec![("Flute", None), ("Trumpet", Some("Brass"))]);
    let trumpet = list.iter().find(|i| i.name == "Trumpet").unwrap().id;
    let flute = list.iter().find(|i| i.name == "Flute").unwrap().id;

    // Only librarians and admins change it.
    let body = json!({"name": "Horn", "family": "Brass"});
    assert_eq!(s.json(Method::POST, "/api/instruments", &lib.anna, &body).await.status, StatusCode::FORBIDDEN);
    let uri = format!("/api/instruments/{flute}");
    assert_eq!(s.json(Method::PUT, &uri, &lib.anna, &body).await.status, StatusCode::FORBIDDEN);
    let reply = s.send(Method::DELETE, &uri, Auth::Bearer(&lib.anna), &[], Body::empty()).await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);

    // Names are unique ignoring case.
    let dup = s.json(Method::POST, "/api/instruments", &lib.admin, json!({"name": "trumpet"})).await;
    assert_eq!(dup.status, StatusCode::CONFLICT);

    // Rename and set the family.
    let renamed: Instrument =
        s.ok(Method::PUT, &uri, &lib.admin, json!({"name": "Flute / Piccolo", "family": "Woodwind"})).await;
    assert_eq!((renamed.name.as_str(), renamed.family.as_deref()), ("Flute / Piccolo", Some("Woodwind")));

    // An instrument used by a part cannot be deleted; an unused one can.
    let used = format!("/api/instruments/{trumpet}");
    let reply = s.send(Method::DELETE, &used, Auth::Bearer(&lib.admin), &[], Body::empty()).await;
    assert_eq!(reply.error(), (StatusCode::CONFLICT, ErrorCode::InUseOrUnknown));
    let horn: Instrument = s.ok(Method::POST, "/api/instruments", &lib.admin, &body).await;
    let reply = s
        .send(Method::DELETE, &format!("/api/instruments/{}", horn.id), Auth::Bearer(&lib.admin), &[], Body::empty())
        .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    let list: Vec<Instrument> = s.get("/api/instruments", &lib.anna).await.json();
    assert_eq!(list.len(), 2);
}

#[tokio::test]
async fn part_names_keys_and_part_search() {
    let s = Server::start().await;
    let lib = library(&s).await;
    let part = |id: Id| format!("/api/parts/{id}");
    let trumpet = lib.trumpet_part.instrument_ids.clone();

    // Voices are free text; common key spellings are normalised; unknown keys are refused.
    let p: Part = s
        .ok(
            Method::PUT,
            &part(lib.trumpet_part.id),
            &lib.admin,
            json!({"instrument_ids": trumpet, "voices": ["1a"], "transposition": "in B-flat"}),
        )
        .await;
    assert_eq!(
        (p.voices.as_slice(), p.transposition.as_deref(), name(&p).as_str()),
        (["1a".to_owned()].as_slice(), Some("Bb"), "Trumpet in B♭ 1a")
    );
    let bad = s
        .json(
            Method::PUT,
            &part(lib.trumpet_part.id),
            &lib.admin,
            json!({"instrument_ids": trumpet, "transposition": "Q"}),
        )
        .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    let bad = s
        .json(
            Method::POST,
            &format!("/api/scores/{}/parts", lib.score),
            &lib.admin,
            json!({"instrument_ids": [], "transposition": "B"}),
        )
        .await;
    assert_eq!(
        bad.error(),
        (StatusCode::BAD_REQUEST, ErrorCode::UnknownKey { key: "B".into() }),
        "B is ambiguous (German B = B-flat)"
    );
    let unnamed: Part = s
        .ok(
            Method::POST,
            &format!("/api/scores/{}/parts", lib.score),
            &lib.admin,
            json!({"instrument_ids": [], "transposition": "H"}),
        )
        .await;
    assert_eq!(name(&unnamed), "Part in B♮");

    // A custom name overrides; clearing it brings the derived name back.
    let p: Part = s
        .ok(
            Method::PUT,
            &part(lib.trumpet_part.id),
            &lib.admin,
            json!({"label": "Solo Cornet", "instrument_ids": trumpet, "voices": ["1"], "transposition": "Bb"}),
        )
        .await;
    assert_eq!((p.label.as_deref(), name(&p).as_str()), (Some("Solo Cornet"), "Solo Cornet"));
    let p: Part = s
        .ok(
            Method::PUT,
            &part(lib.trumpet_part.id),
            &lib.admin,
            json!({"label": "", "instrument_ids": trumpet, "voices": [1], "transposition": "Bb"}),
        )
        .await;
    assert_eq!((p.label.as_deref(), name(&p).as_str()), (None, "Trumpet in B♭ 1"));

    // Parts of a score are ordered by name, numbers by value.
    for v in ["10", "2"] {
        s.ok::<Part>(
            Method::POST,
            &format!("/api/scores/{}/parts", lib.score),
            &lib.admin,
            json!({"instrument_ids": trumpet, "voices": [v], "transposition": "Bb"}),
        )
        .await;
    }
    let score: Score = s.get(&format!("/api/scores/{}", lib.score), &lib.anna).await.json();
    let names = names(&score.parts);
    assert_eq!(names, ["Trumpet in B♭ 1", "Trumpet in B♭ 2", "Trumpet in B♭ 10"]);

    // The library search finds pieces by the names of the user's own parts,
    // also mixed with score words, but not by parts the user cannot read.
    let (anna, admin) = (lib.anna.as_str(), lib.admin.as_str());
    assert_eq!(s.hits("trumpet", anna).await, 1);
    assert_eq!(s.hits("fucik%20trump", anna).await, 1);
    assert_eq!(s.hits("bb", anna).await, 1, "key code is searchable");
    // Components and key spellings (R-I18N-007): the composed name, the code
    // and the glyph form all find the part; the connecting word is ignored.
    assert_eq!(s.hits("trumpet%20in%20B%E2%99%AD%202", anna).await, 1, "Trumpet in B♭ 2");
    assert_eq!(s.hits("trumpet%20bb%2010", anna).await, 1);
    assert_eq!(s.hits("trumpet%20B%2010", anna).await, 1, "B♭ is indexed as the token b");
    assert_eq!(s.hits("trumpet%20in", anna).await, 1, "a trailing connector is dropped");
    assert_eq!(s.hits("trumpet%20eb", anna).await, 0);
    assert_eq!(s.hits("solo%20flute", anna).await, 0, "anna cannot read the flute part");
    assert_eq!(s.hits("solo%20flute", admin).await, 1);
    assert_eq!(s.hits("trumpet%20concerto", anna).await, 0, "every word must match");

    // Renaming an instrument renames derived part names and re-indexes them.
    let trumpet_id = trumpet[0];
    s.ok::<Instrument>(
        Method::PUT,
        &format!("/api/instruments/{trumpet_id}"),
        &lib.admin,
        json!({"name": "Cornet", "family": "Brass"}),
    )
    .await;
    let p: Part = s.get(&part(lib.trumpet_part.id), &lib.anna).await.json();
    assert_eq!(name(&p), "Cornet in B♭ 1");
    assert_eq!(s.hits("cornet", anna).await, 1);
    assert_eq!(s.hits("trumpet", anna).await, 0);

    // Deleting a part removes it from the index.
    s.send(Method::DELETE, &part(lib.trumpet_part.id), Auth::Bearer(&lib.admin), &[], Body::empty()).await;
    s.send(Method::DELETE, &part(unnamed.id), Auth::Bearer(&lib.admin), &[], Body::empty()).await;
}

/// Several voices per part (a-part-voices): order kept, duplicates and
/// blanks dropped, limits enforced, named "… 1 & 2", found under each voice.
#[tokio::test]
async fn parts_with_several_voices() {
    let s = Server::start().await;
    let lib = library(&s).await;
    let trumpet = lib.trumpet_part.instrument_ids.clone();
    let new_part = format!("/api/scores/{}/parts", lib.score);
    let p: Part = s
        .ok(
            Method::POST,
            &new_part,
            &lib.admin,
            json!({"instrument_ids": trumpet, "voices": [" 2 ", 1, "", "2", "solo", "Solo"], "transposition": "Bb"}),
        )
        .await;
    assert_eq!(p.voices, ["2", "1", "solo"], "order kept, blanks and duplicates (ignoring case) dropped");
    assert_eq!(name(&p), "Trumpet in B♭ 2 & 1 & solo");
    // Update replaces the whole list, in the new order.
    let p: Part = s
        .ok(
            Method::PUT,
            &format!("/api/parts/{}", p.id),
            &lib.admin,
            json!({"instrument_ids": trumpet, "voices": ["1", "2"], "transposition": "Bb"}),
        )
        .await;
    assert_eq!(
        (p.voices.as_slice(), name(&p).as_str()),
        (["1", "2"].map(String::from).as_slice(), "Trumpet in B♭ 1 & 2")
    );
    // "&" separates voices, so a typed "3 & 4" is two voices.
    let q: Part =
        s.ok(Method::POST, &new_part, &lib.admin, json!({"instrument_ids": trumpet, "voices": ["3 & 4"]})).await;
    assert_eq!(q.voices, ["3", "4"]);
    // Ordered by name: the first voice, then the rest.
    let score: Score = s.get(&format!("/api/scores/{}", lib.score), &lib.anna).await.json();
    let names = names(&score.parts);
    assert_eq!(names, ["Trumpet 3 & 4", "Trumpet in B♭ 1 & 2", "Trumpet in B♭ 2"]);
    let got: Part = s.get(&format!("/api/parts/{}", p.id), &lib.anna).await.json();
    assert_eq!(got.voices, ["1", "2"], "members see the voices");
    // Limits: at most 10 voices of at most 20 characters.
    let many: Vec<String> = (1..=11).map(|i| i.to_string()).collect();
    let bad = s.json(Method::POST, &new_part, &lib.admin, json!({"instrument_ids": trumpet, "voices": many})).await;
    assert_eq!(bad.error(), (StatusCode::BAD_REQUEST, ErrorCode::TooManyVoices { max: 10 }));
    let bad = s
        .json(Method::POST, &new_part, &lib.admin, json!({"instrument_ids": trumpet, "voices": ["x".repeat(21)]}))
        .await;
    assert_eq!(bad.error(), (StatusCode::BAD_REQUEST, ErrorCode::VoiceTooLong { voice: "x".repeat(21), max: 20 }));
    // Found under each voice.
    let anna = lib.anna.as_str();
    assert_eq!(s.hits("trumpet%201", anna).await, 1);
    assert_eq!(s.hits("trumpet%204", anna).await, 1);
    let stored: Vec<(i64, String)> =
        sqlx::query_as("SELECT position, voice FROM part_voices WHERE part_id = ?1 ORDER BY position")
            .bind(p.id)
            .fetch_all(&s.state.db.read)
            .await
            .unwrap();
    assert_eq!(stored, [(0, "1".to_owned()), (1, "2".to_owned())], "stored relationally, in order");
    // Deleting the part deletes its voices.
    s.send(Method::DELETE, &format!("/api/parts/{}", p.id), Auth::Bearer(&lib.admin), &[], Body::empty()).await;
    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM part_voices WHERE part_id = ?1")
        .bind(p.id)
        .fetch_one(&s.state.db.read)
        .await
        .unwrap();
    assert_eq!(left, 0);
}

/// Migration 0009 (a-client-part-names): sets named "My annotations" become unnamed (NULL);
/// the table rebuild keeps their pages, foreign keys are on again, one unnamed set per part.
#[tokio::test]
async fn migration_0009_unnames_default_sets_and_keeps_their_pages() {
    let dir = tempfile::tempdir().unwrap();
    let old = tempfile::tempdir().unwrap();
    for f in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/migrations")).unwrap() {
        let f = f.unwrap();
        if *f.file_name().to_string_lossy() < *"0009" {
            std::fs::copy(f.path(), old.path().join(f.file_name())).unwrap();
        }
    }
    {
        let opts = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(dir.path().join("musicus.sqlite"))
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = sqlx::SqlitePool::connect_with(opts).await.unwrap();
        sqlx::migrate::Migrator::new(old.path()).await.unwrap().run(&pool).await.unwrap();
        sqlx::raw_sql(
            "INSERT INTO users (id, username, display_name, role, created_at) VALUES (1, 'anna', 'Anna', 'admin', 0);
             INSERT INTO scores (id, title, tags, created_at, updated_at) VALUES (1, 'March', '[]', 0, 0);
             INSERT INTO parts (id, score_id, created_at) VALUES (1, 1, 0);
             INSERT INTO files (id, sha256, mime, size_bytes, created_at) VALUES (1, 'aa', 'application/pdf', 1, 0);
             INSERT INTO part_files (part_id, version, file_id, uploaded_at) VALUES (1, 1, 1, 0);
             INSERT INTO annotation_sets (id, user_id, part_id, file_id, name, is_default, created_at, updated_at)
               VALUES (1, 1, 1, 1, 'My annotations', 1, 0, 0), (2, 1, 1, 1, 'Concert', 0, 0, 0);
             INSERT INTO annotation_pages (set_id, page, revision, data, updated_at) VALUES (1, 0, 3, x'00', 0);",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;
    }
    let settings = Settings { cookie_secure: false, ..Settings::default() };
    let state = open_state(dir.path(), 1 << 20, settings).await.unwrap();
    let rows: Vec<(Id, Option<String>, i64)> = sqlx::query_as(
        "SELECT s.id, s.name, (SELECT COUNT(*) FROM annotation_pages p WHERE p.set_id = s.id) FROM annotation_sets s ORDER BY s.id",
    )
    .fetch_all(&state.db.read)
    .await
    .unwrap();
    assert_eq!(rows, [(1, None, 1), (2, Some("Concert".to_owned()), 0)], "pages survive the table rebuild");
    let fk: bool = sqlx::query_scalar("PRAGMA foreign_keys").fetch_one(&state.db.write).await.unwrap();
    assert!(fk, "foreign keys are on again");
    // One unnamed set per user and part.
    let dup = sqlx::query(
        "INSERT INTO annotation_sets (user_id, part_id, file_id, name, created_at, updated_at) VALUES (1, 1, 1, NULL, 0, 0)",
    )
    .execute(&state.db.write)
    .await;
    assert!(dup.is_err());
}

/// Data written before migration 0002: labels become custom names, numeric
/// voices text, known key spellings codes; unknown keys stay and can be kept
/// on update; the part search index is built on the first start. Migration
/// 0005 then makes each old voice the only entry of the part's voice list.
#[tokio::test]
async fn migration_0002_keeps_old_parts() {
    let dir = tempfile::tempdir().unwrap();
    let old = tempfile::tempdir().unwrap();
    std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/migrations/0001_init.sql"), old.path().join("0001_init.sql"))
        .unwrap();
    {
        let opts = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(dir.path().join("musicus.sqlite"))
            .create_if_missing(true);
        let pool = sqlx::SqlitePool::connect_with(opts).await.unwrap();
        sqlx::migrate::Migrator::new(old.path()).await.unwrap().run(&pool).await.unwrap();
        sqlx::raw_sql(
            "INSERT INTO instruments (id, name) VALUES (1, 'Clarinet'), (2, 'Horn');
             INSERT INTO scores (id, title, tags, created_at, updated_at) VALUES (1, 'Old March', '[]', 0, 0);
             INSERT INTO parts (id, score_id, label, voice, transposition, created_at) VALUES
               (1, 1, '2nd Clarinet in Bb', 2, 'in Bb', 0),
               (2, 1, 'Horn', NULL, 'Es', 0),
               (3, 1, '  ', NULL, 'B', 0),
               (4, 1, 'Horn 2', 2, 'Es/F', 0),
               (5, 1, 'Horn 3', 3, '', 0);
             INSERT INTO part_instruments (part_id, instrument_id) VALUES (1, 1), (2, 2), (3, 1), (4, 2), (5, 2);",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;
    }
    let settings = Settings { cookie_secure: false, ..Settings::default() };
    let state = open_state(dir.path(), 1 << 20, settings).await.unwrap();
    let invite = bootstrap_admin(&state).await.unwrap().unwrap();
    let s = Server { router: routes::app(state.clone(), None), state, _dir: dir, admin_invite: invite };
    let admin = s.admin().await;
    let score: Score = s.get("/api/scores/1", &admin).await.json();
    type Row<'a> = (Option<&'a str>, Vec<&'a str>, Option<&'a str>, String);
    let parts = sorted(&score.parts);
    let got: Vec<Row> = parts
        .iter()
        .map(|p| {
            let voices = p.voices.iter().map(String::as_str).collect();
            (p.label.as_deref(), voices, p.transposition.as_deref(), name(p))
        })
        .collect();
    assert_eq!(
        got,
        [
            (Some("2nd Clarinet in Bb"), vec!["2"], Some("Bb"), "2nd Clarinet in Bb".to_owned()),
            (None, vec![], Some("B"), "Clarinet in B".to_owned()),
            (Some("Horn"), vec![], Some("Eb"), "Horn".to_owned()),
            (Some("Horn 2"), vec!["2"], Some("Es/F"), "Horn 2".to_owned()),
            (Some("Horn 3"), vec!["3"], None, "Horn 3".to_owned()),
        ]
    );
    // The unknown key can be kept, but not set anew.
    let keep = s
        .json(
            Method::PUT,
            "/api/parts/4",
            &admin,
            json!({"label": "Horn 2", "instrument_ids": [2], "voices": ["2"], "transposition": "Es/F"}),
        )
        .await;
    assert_eq!(keep.status, StatusCode::OK);
    let set =
        s.json(Method::PUT, "/api/parts/5", &admin, json!({"instrument_ids": [2], "transposition": "Es/F"})).await;
    assert_eq!(set.status, StatusCode::BAD_REQUEST);
    // Index built on start: old parts are found by their (derived) names.
    assert_eq!(s.get("/api/scores?q=clarinet", &admin).await.json::<Vec<Score>>().len(), 1);
    assert_eq!(s.get("/api/scores?q=horn%203", &admin).await.json::<Vec<Score>>().len(), 1);
    // Migration 0006 rebuilt the score index: old titles are still found.
    assert_eq!(s.get("/api/scores?q=old%20mar", &admin).await.json::<Vec<Score>>().len(), 1);
    assert_eq!(score.archive_number, None);
}

#[tokio::test]
async fn archive_numbers_are_free_text_searchable_and_not_unique() {
    let s = Server::start().await;
    let lib = library(&s).await;
    let a: Score = s
        .ok(Method::POST, "/api/scores", &lib.admin, json!({"title": "Alte Kameraden", "archive_number": " 0304 "}))
        .await;
    assert_eq!(a.archive_number.as_deref(), Some("0304"), "trimmed");
    // Not unique: a second piece may carry the same number (gaps and typos in real archives).
    let b: Score =
        s.ok(Method::POST, "/api/scores", &lib.admin, json!({"title": "Second copy", "archive_number": "0304"})).await;
    assert_eq!(b.archive_number.as_deref(), Some("0304"));
    for q in ["0304", "304", "alte%200304"] {
        assert!(s.hits(q, &lib.admin).await >= 1, "{q}");
    }
    assert_eq!(s.hits("304", &lib.admin).await, 2);
    // Numbers with separators are found without them too.
    let c: Score = s
        .ok(
            Method::PUT,
            &format!("/api/scores/{}", b.id),
            &lib.admin,
            json!({"title": "Second copy", "archive_number": "A-12"}),
        )
        .await;
    assert_eq!(c.archive_number.as_deref(), Some("A-12"));
    assert_eq!(s.hits("A12", &lib.admin).await, 1);
    assert_eq!(s.hits("a-12", &lib.admin).await, 1);
    assert_eq!(s.hits("304", &lib.admin).await, 1, "the old number is gone from the index");
    // Blank = none; too long = 400.
    let cleared: Score = s
        .ok(Method::PUT, &format!("/api/scores/{}", c.id), &lib.admin, json!({"title": "x", "archive_number": " "}))
        .await;
    assert_eq!(cleared.archive_number, None);
    let long =
        s.json(Method::POST, "/api/scores", &lib.admin, json!({"title": "x", "archive_number": "9".repeat(41)})).await;
    assert_eq!(
        long.error(),
        (StatusCode::BAD_REQUEST, ErrorCode::FieldTooLong { field: Field::ArchiveNumber, max: 40 })
    );
    // Members see the number of the pieces they can read, and find them by it.
    s.ok::<Score>(
        Method::PUT,
        &format!("/api/scores/{}", lib.score),
        &lib.admin,
        json!({"title": "Florentiner Marsch", "composer": "Julius Fučík", "archive_number": "17"}),
    )
    .await;
    let mine: Vec<Score> = s.get("/api/scores?q=17", &lib.anna).await.json();
    assert_eq!(mine.iter().map(|s| s.archive_number.as_deref()).collect::<Vec<_>>(), [Some("17")]);
    assert_eq!(s.hits("304", &lib.anna).await, 0, "no parts of Alte Kameraden for anna");
}

#[tokio::test]
async fn system_layouts_are_detected_in_the_background_and_follow_file_access() {
    use musicus_api::{
        CurrentUser,
        layout::{FileLayout, LayoutState, ReaderMode, ReaderSettings, RegionKind, StaffSize},
    };
    use musicus_server::layout::worker;
    let s = Server::start().await;
    let lib = library(&s).await;
    let part: Part = s.upload(&lib.admin, lib.trumpet_part.id, staff_pdf(2, 8)).await.json();
    let sha = part.file.unwrap().sha256;
    let flute_sha = lib.flute_part.file.as_ref().unwrap().sha256.clone();

    // Until the worker ran, the layout is pending (requests never wait for it).
    let pending: FileLayout = s.get(&format!("/api/files/{sha}/layout"), &lib.anna).await.json();
    assert_eq!(pending.state, LayoutState::Pending);
    // The worker analyses every file without a layout (the three uploads), newest first.
    let mut runs = 0;
    while worker::run_once(&s.state).await.unwrap() {
        runs += 1;
    }
    assert_eq!(runs, 3);
    let layout: FileLayout = s.get(&format!("/api/files/{sha}/layout"), &lib.anna).await.json();
    assert_eq!(layout.state, LayoutState::Done);
    assert_eq!(layout.algorithm, musicus_server::layout::ALGORITHM_VERSION);
    assert_eq!(layout.pages.len(), 2);
    for page in &layout.pages {
        let systems: Vec<_> = page.regions.iter().filter(|r| r.kind == RegionKind::System).collect();
        assert_eq!(systems.len(), 8, "{:?}", page.regions);
        assert!(systems.iter().all(|r| r.staves == 1) && !page.full_page);
        assert!((page.width_mm - 209.9).abs() < 0.5);
        // Line distance 5 pt of 842 pt.
        assert!((systems[0].staff_space - 5.0 / 842.0).abs() < 0.001);
    }
    // Empty pages: analysed, shown whole.
    let empty: FileLayout = s.get(&format!("/api/files/{flute_sha}/layout"), &lib.admin).await.json();
    assert!(empty.state == LayoutState::Done && empty.pages.iter().all(|p| p.full_page));

    // Same access rule as the file: denied (or unknown) answers 404.
    assert_eq!(s.get(&format!("/api/files/{flute_sha}/layout"), &lib.anna).await.status, StatusCode::NOT_FOUND);
    assert_eq!(s.get(&format!("/api/files/{}/layout", "0".repeat(64)), &lib.anna).await.status, StatusCode::NOT_FOUND);
    assert_eq!(s.get("/api/files/xyz/layout", &lib.anna).await.status, StatusCode::NOT_FOUND);

    // Reader settings live on the server, per user.
    let me: CurrentUser = s.get("/api/auth/me", &lib.anna).await.json();
    assert_eq!(me.reader, ReaderSettings { mode: ReaderMode::Page, staff_size: StaffSize::M });
    let new = ReaderSettings { mode: ReaderMode::Systems, staff_size: StaffSize::XL };
    let saved: ReaderSettings = s.ok(Method::PUT, "/api/auth/reader-settings", &lib.anna, new).await;
    assert_eq!(saved, new);
    assert_eq!(s.get("/api/auth/me", &lib.anna).await.json::<CurrentUser>().reader, new);
    assert_eq!(s.get("/api/auth/me", &lib.admin).await.json::<CurrentUser>().reader.mode, ReaderMode::Page);
    let bad =
        s.json(Method::PUT, "/api/auth/reader-settings", &lib.anna, json!({"mode": "scroll", "staff_size": "M"})).await;
    assert!(bad.status.is_client_error());
}

#[tokio::test]
async fn librarians_correct_system_layouts_for_everyone() {
    use musicus_api::layout::{FileLayout, LayoutState, PageCorrection, RegionKind};
    use musicus_server::layout::worker;
    let s = Server::start().await;
    let lib = library(&s).await;
    let part: Part = s.upload(&lib.admin, lib.trumpet_part.id, staff_pdf(2, 8)).await.json();
    let sha = part.file.unwrap().sha256;
    let uri = |page: i64| format!("/api/files/{sha}/layout/pages/{page}");
    let merge = |l: &FileLayout| {
        // Page 1: merge the first two systems, hide everything below the fourth.
        let sys: Vec<_> = l.pages[0].regions.iter().filter(|r| r.kind == RegionKind::System).collect();
        PageCorrection {
            full_page: false,
            bounds: vec![sys[0].y0, sys[1].y1, sys[3].y1, sys[7].y1],
            kinds: vec![RegionKind::System, RegionKind::System, RegionKind::Hidden],
        }
    };

    // Not analysed yet: nothing to correct.
    let c = PageCorrection { full_page: true, ..Default::default() };
    assert_eq!(
        s.json(Method::PUT, &uri(0), &lib.admin, &c).await.error(),
        (StatusCode::CONFLICT, ErrorCode::SystemsNotDetected)
    );
    while worker::run_once(&s.state).await.unwrap() {}
    let auto: FileLayout = s.get(&format!("/api/files/{sha}/layout"), &lib.anna).await.json();

    // Members may not correct; bad corrections and pages are rejected.
    assert_eq!(s.json(Method::PUT, &uri(0), &lib.anna, &c).await.status, StatusCode::FORBIDDEN);
    assert_eq!(
        s.send(Method::DELETE, &uri(0), Auth::Bearer(&lib.anna), &[], Body::empty()).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(s.json(Method::PUT, &uri(2), &lib.admin, &c).await.status, StatusCode::NOT_FOUND);
    let bad = PageCorrection { full_page: false, bounds: vec![0.5, 0.2], kinds: vec![RegionKind::System] };
    assert_eq!(
        s.json(Method::PUT, &uri(0), &lib.admin, &bad).await.error(),
        (StatusCode::BAD_REQUEST, ErrorCode::CorrectionBoundsOrder)
    );

    // A correction applies to every member's layout.
    let corrected: FileLayout = s.ok(Method::PUT, &uri(0), &lib.admin, merge(&auto)).await;
    let page = &corrected.pages[0];
    assert!(page.corrected && !page.full_page);
    let kinds: Vec<(RegionKind, u32)> = page.regions.iter().map(|r| (r.kind, r.staves)).collect();
    assert_eq!(kinds, vec![(RegionKind::System, 2), (RegionKind::System, 2), (RegionKind::Hidden, 0)]);
    s.ok::<FileLayout>(Method::PUT, &uri(1), &lib.admin, PageCorrection { full_page: true, ..Default::default() })
        .await;
    let seen: FileLayout = s.get(&format!("/api/files/{sha}/layout"), &lib.anna).await.json();
    assert_eq!(seen.pages[0], *page);
    assert!(seen.pages[1].full_page && seen.pages[1].corrected);

    // A new detection run (algorithm raised) keeps the corrections.
    sqlx::query("UPDATE file_layouts SET algorithm = 0").execute(&s.state.db.write).await.unwrap();
    let mut runs = 0;
    while worker::run_once(&s.state).await.unwrap() {
        runs += 1;
    }
    assert_eq!(runs, 3);
    let rerun: FileLayout = s.get(&format!("/api/files/{sha}/layout"), &lib.anna).await.json();
    assert_eq!(rerun.state, LayoutState::Done);
    assert_eq!(rerun.pages[0], *page);

    // Reset: back to the automatic detection.
    let reset = s.send(Method::DELETE, &uri(0), Auth::Bearer(&lib.admin), &[], Body::empty()).await;
    assert_eq!(reset.status, StatusCode::OK);
    let reset: FileLayout = reset.json();
    assert_eq!(reset.pages[0], auto.pages[0]);
    assert!(reset.pages[1].corrected);
}

// ---------------------------------------------------------------------------
// Mass import (F-LIB-004)
// ---------------------------------------------------------------------------

fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(bytes))
}

/// Registers files (path, content) with a batch; returns the files to upload by path.
async fn register(s: &Server, token: &str, batch: Id, files: &[(&str, &[u8])]) -> RegisteredImportFiles {
    let list: Vec<Value> = files
        .iter()
        .map(|(path, bytes)| json!({"path": path, "size_bytes": bytes.len(), "sha256": sha256(bytes)}))
        .collect();
    s.ok(Method::POST, &format!("/api/import/batches/{batch}/files"), token, json!({ "files": list })).await
}

async fn upload_all(s: &Server, token: &str, todo: &RegisteredImportFiles, files: &[(&str, &[u8])]) {
    for u in &todo.upload {
        let bytes = files.iter().find(|(p, _)| *p == u.path).unwrap().1.to_vec();
        let r = s
            .send(
                Method::PUT,
                &format!("/api/import/files/{}/content", u.id),
                Auth::Bearer(token),
                &[],
                Body::from(bytes),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{}: {}", u.path, String::from_utf8_lossy(&r.body));
    }
}

fn piece<'a>(review: &'a ImportReview, title: &str) -> &'a ReviewPiece {
    review.pieces.iter().find(|p| p.title.value.as_deref() == Some(title)).unwrap_or_else(|| panic!("no piece {title}"))
}

fn part<'a>(piece: &'a ReviewPiece, text: &str) -> &'a ReviewPart {
    piece.parts.iter().find(|p| p.text == text).unwrap_or_else(|| panic!("no part {text}"))
}

#[tokio::test]
async fn import_stages_reviews_commits_and_undoes() {
    let s = Server::start().await;
    let lib = library(&s).await;
    let admin = &lib.admin;

    // Members cannot use the import at all.
    assert_eq!(s.get("/api/import/batches", &lib.anna).await.status, StatusCode::FORBIDDEN);
    let folder = json!({"name": "Archiv", "pattern": PRESETS[0].pattern, "language": "de"});
    assert_eq!(s.json(Method::POST, "/api/import/batches", &lib.anna, &folder).await.status, StatusCode::FORBIDDEN);
    assert_eq!(s.get("/api/import/aliases", &lib.anna).await.status, StatusCode::FORBIDDEN);
    let bad = json!({"name": "x", "pattern": "{title}/{foo}.pdf", "language": "de"});
    assert_eq!(
        s.json(Method::POST, "/api/import/batches", admin, bad).await.error(),
        (StatusCode::BAD_REQUEST, ErrorCode::InvalidPattern { reason: PatternError::UnknownPlaceholder("foo".into()) })
    );

    let batch: ImportBatch = s.ok(Method::POST, "/api/import/batches", admin, &folder).await;
    let trumpet = tiny_pdf(2, "import-trumpet");
    let partitur = tiny_pdf(3, "import-partitur");
    let direktion = tiny_pdf(2, "import-direktion");
    let klar = tiny_pdf(1, "import-klar");
    let flute = tiny_pdf(1, "import-flute");
    let in_library = tiny_pdf(4, "trumpet"); // the fixture's trumpet part
    let dir = "Märsche/0304 Alte Kameraden - Teike";
    let files: Vec<(String, Vec<u8>)> = vec![
        (format!("{dir}/Trompete 1 in B.pdf"), trumpet.clone()),
        (format!("{dir}/Partitur.pdf"), partitur.clone()),
        (format!("{dir}/Direktion.pdf"), direktion.clone()),
        (format!("{dir}/Klar 2.pdf"), klar.clone()),
        ("Florentiner Marsch - Julius Fučík/Flöte.pdf".into(), flute.clone()),
        ("0305 Böhmischer Traum/Trompete 2.pdf".into(), in_library.clone()),
        ("Zweitschrift/Trompete.pdf".into(), trumpet.clone()),
        ("lose.pdf".into(), tiny_pdf(1, "loose")),
    ];
    let files: Vec<(&str, &[u8])> = files.iter().map(|(p, b)| (p.as_str(), b.as_slice())).collect();
    let todo = register(&s, admin, batch.id, &files).await;
    // Content already in the library or twice in the folder is skipped.
    assert_eq!(todo.files.duplicate, 2);
    assert_eq!(todo.upload.len(), 6);
    assert!(!todo.upload.iter().any(|u| u.path.contains("Zweitschrift") || u.path.contains("0305")));

    // A file whose content differs from the announced hash is rejected.
    let first = &todo.upload[0];
    let wrong = s
        .send(
            Method::PUT,
            &format!("/api/import/files/{}/content", first.id),
            Auth::Bearer(admin),
            &[],
            Body::from(tiny_pdf(1, "other")),
        )
        .await;
    assert_eq!(wrong.error(), (StatusCode::UNPROCESSABLE_ENTITY, ErrorCode::FileChanged));
    // Resume: registering the same folder again asks for exactly the missing files.
    let again = register(&s, admin, batch.id, &files).await;
    assert_eq!(again.upload.len(), 6);
    upload_all(&s, admin, &again, &files).await;
    assert!(register(&s, admin, batch.id, &files).await.upload.is_empty(), "nothing to upload twice");

    // Staged files are not part of the library: nobody gets them through /api/files.
    let trumpet_sha = sha256(&trumpet);
    for token in [admin.as_str(), lib.anna.as_str()] {
        assert_eq!(s.get(&format!("/api/files/{trumpet_sha}"), token).await.status, StatusCode::NOT_FOUND);
    }
    let review: ImportReview = s.get(&format!("/api/import/batches/{}", batch.id), admin).await.json();
    assert_eq!(review.unmatched.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["lose.pdf"]);
    assert_eq!(review.duplicates.len(), 2);
    let alte = piece(&review, "Alte Kameraden");
    let staged = part(alte, "Trompete 1 in B").file.id;
    assert_eq!(s.get(&format!("/api/import/files/{staged}"), admin).await.body, trumpet);
    let thumb = s.get(&format!("/api/import/files/{staged}/thumbnail"), admin).await;
    assert_eq!((thumb.status, thumb.headers[header::CONTENT_TYPE].to_str().unwrap()), (StatusCode::OK, "image/png"));
    assert_eq!(s.get(&format!("/api/import/files/{staged}/thumbnail"), &lib.anna).await.status, StatusCode::FORBIDDEN);

    // Review: number, title, composer, genre from the folder names; key B = B♭ in a German batch.
    assert_eq!(
        (alte.number.value.as_deref(), alte.composer.value.as_deref(), alte.genre.value.as_deref()),
        (Some("0304"), Some("Teike"), Some("Märsche"))
    );
    let t = part(alte, "Trompete 1 in B");
    assert_eq!(
        (review_name(t).as_str(), t.level, t.readers.clone()),
        ("Trumpet in B♭ 1", Level::Green, vec!["Trumpets".to_string()])
    );
    // Unknown instruments are red and are never created by the import.
    for text in ["Partitur", "Direktion", "Klar 2"] {
        assert_eq!(part(alte, text).level, Level::Red, "{text}");
    }
    assert_eq!(alte.level, Level::Red);
    let refused: CommittedImport = s
        .ok(Method::POST, &format!("/api/import/batches/{}/commit", batch.id), admin, json!({"piece_ids": [alte.id]}))
        .await;
    assert!(refused.committed.is_empty());
    assert!(
        matches!(&refused.errors[0].code, ErrorCode::ImportBlocked { note: Some(Note::UnknownInstrument(_)) }),
        "{:?}",
        refused.errors[0]
    );

    // The librarian creates Partitur and Direktion (two instruments) and maps "Klar" once.
    let score_instr: Instrument = s.ok(Method::POST, "/api/instruments", admin, json!({"name": "Partitur"})).await;
    let cond_instr: Instrument = s.ok(Method::POST, "/api/instruments", admin, json!({"name": "Direktion"})).await;
    let clarinet: Instrument = s.ok(Method::POST, "/api/instruments", admin, json!({"name": "Klarinette"})).await;
    let review: ImportReview = s.get(&format!("/api/import/batches/{}", batch.id), admin).await.json();
    let alte = piece(&review, "Alte Kameraden");
    assert_eq!(part(alte, "Partitur").instruments[0].instrument_id, Some(score_instr.id));
    assert_eq!(part(alte, "Direktion").instruments[0].instrument_id, Some(cond_instr.id));
    let k = part(alte, "Klar 2");
    assert_eq!((k.instruments[0].instrument_id, k.level), (Some(clarinet.id), Level::Yellow), "abbreviation: check");
    let alias: InstrumentAlias =
        s.ok(Method::PUT, "/api/import/aliases", admin, json!({"alias": "Klar", "instrument_id": clarinet.id})).await;
    assert_eq!(alias.alias, "klar");
    let review: ImportReview = s.get(&format!("/api/import/batches/{}", batch.id), admin).await.json();
    let alte = piece(&review, "Alte Kameraden");
    assert_eq!((part(alte, "Klar 2").level, alte.level), (Level::Green, Level::Green));

    // An existing piece with the same title and composer is only suggested.
    let flor = piece(&review, "Florentiner Marsch");
    assert_eq!((flor.suggestion.as_ref().map(|s| s.id), flor.level), (Some(lib.score), Level::Yellow));
    let flute_part = part(flor, "Flöte");
    assert_eq!(flute_part.readers, Vec::<String>::new(), "nobody has the flute");

    // Commit all green: only Alte Kameraden.
    let done: CommittedImport =
        s.ok(Method::POST, &format!("/api/import/batches/{}/commit", batch.id), admin, json!({})).await;
    assert_eq!((done.committed.clone(), done.errors.len()), (vec![alte.id], 0));
    let mine: Vec<Score> = s.get("/api/scores?q=304", &lib.anna).await.json();
    assert_eq!(mine.len(), 1, "anna finds the new piece by its number");
    assert_eq!(names(&mine[0].parts), ["Trumpet in B♭ 1"]);
    assert_eq!(s.get(&format!("/api/files/{trumpet_sha}"), &lib.anna).await.status, StatusCode::OK);
    let all: Score = s.get(&format!("/api/scores/{}", mine[0].id), admin).await.json();
    assert_eq!(all.parts.len(), 4);
    assert_eq!(all.archive_number.as_deref(), Some("0304"));
    assert!(
        all.parts.iter().any(|p| p.instrument_ids == [score_instr.id])
            && all.parts.iter().any(|p| p.instrument_ids == [cond_instr.id])
    );

    // Confirmed: the flute part goes to the existing piece.
    let edited: Vec<ReviewPiece> = s
        .ok(
            Method::PATCH,
            "/api/import/items",
            admin,
            json!({"ids": [flor.id], "piece": {"target": {"existing": lib.score}}}),
        )
        .await;
    assert_eq!((edited[0].level, edited[0].score.as_ref().map(|s| s.id)), (Level::Green, Some(lib.score)));
    let done: CommittedImport = s
        .ok(Method::POST, &format!("/api/import/batches/{}/commit", batch.id), admin, json!({"piece_ids": [flor.id]}))
        .await;
    assert_eq!(done.committed, [flor.id]);
    let florentiner: Score = s.get(&format!("/api/scores/{}", lib.score), admin).await.json();
    assert_eq!(florentiner.parts.len(), 3);
    // Committed items cannot be edited any more.
    let locked =
        s.json(Method::PATCH, "/api/import/items", admin, json!({"ids": [flor.id], "piece": {"title": "x"}})).await;
    assert_eq!(locked.error(), (StatusCode::CONFLICT, ErrorCode::ImportCommitted));

    // Undo is refused while a musician has annotated an imported part.
    let imported = mine[0].parts[0].id;
    let set: AnnotationSet =
        s.ok(Method::POST, &format!("/api/parts/{imported}/annotation-sets"), &lib.anna, json!({"name": "Mine"})).await;
    let undo = s.json(Method::POST, &format!("/api/import/batches/{}/undo", batch.id), admin, json!({})).await;
    let (status, code) = undo.error();
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(matches!(&code, ErrorCode::UndoAnnotated { pieces } if pieces.iter().any(|p| p == "Alte Kameraden")));
    let r = s
        .send(Method::DELETE, &format!("/api/annotation-sets/{}", set.id), Auth::Bearer(&lib.anna), &[], Body::empty())
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let undone: UndoneImport =
        s.ok(Method::POST, &format!("/api/import/batches/{}/undo", batch.id), admin, json!({})).await;
    assert_eq!((undone.pieces, undone.parts), (1, 5));
    assert_eq!(s.get(&format!("/api/scores/{}", mine[0].id), admin).await.status, StatusCode::NOT_FOUND);
    assert_eq!(
        s.get(&format!("/api/scores/{}", lib.score), admin).await.json::<Score>().parts.len(),
        2,
        "existing piece kept"
    );
    // The files stay in the store; the batch can be committed again.
    assert_eq!(s.get(&format!("/api/import/files/{staged}"), admin).await.status, StatusCode::OK);
    let review: ImportReview = s.get(&format!("/api/import/batches/{}", batch.id), admin).await.json();
    assert_eq!(piece(&review, "Alte Kameraden").state, ItemState::Pending);
    let batches: Vec<ImportBatch> = s.get("/api/import/batches", admin).await.json();
    assert_eq!((batches[0].pieces, batches[0].committed, batches[0].files.unmatched), (2, 0, 1));

    // Aliases can be listed and removed; deleting a batch keeps the library.
    assert_eq!(s.get("/api/import/aliases", admin).await.json::<Vec<InstrumentAlias>>().len(), 1);
    let r = s.send(Method::DELETE, "/api/import/aliases?alias=Klar", Auth::Bearer(admin), &[], Body::empty()).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let r = s
        .send(Method::DELETE, &format!("/api/import/batches/{}", batch.id), Auth::Bearer(admin), &[], Body::empty())
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(s.get(&format!("/api/import/batches/{}", batch.id), admin).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn import_of_flat_numbered_files_groups_by_number_and_keeps_changes() {
    let s = Server::start().await;
    let lib = library(&s).await;
    let admin = &lib.admin;
    let klarinette: Instrument = s.ok(Method::POST, "/api/instruments", admin, json!({"name": "Klarinette"})).await;
    let batch: ImportBatch = s
        .ok(
            Method::POST,
            "/api/import/batches",
            admin,
            json!({"name": "Scans", "pattern": preset("flat-number").unwrap().pattern, "language": "en"}),
        )
        .await;
    let (a, b, c, d) = (tiny_pdf(1, "k2"), tiny_pdf(1, "keb"), tiny_pdf(1, "k28"), tiny_pdf(1, "k3"));
    // Archive number and part with different separators.
    let files: Vec<(&str, &[u8])> = vec![
        ("304_Klarinette 2.pdf", &a),
        ("0304 - Klarinette in B.pdf", &b),
        ("28 Klarinette Eb.pdf", &c),
        ("0304 Klarinette Klar 3.pdf", &d),
    ];
    let todo = register(&s, admin, batch.id, &files).await;
    upload_all(&s, admin, &todo, &files).await;
    let review: ImportReview = s.get(&format!("/api/import/batches/{}", batch.id), admin).await.json();
    assert_eq!(review.pieces.len(), 2, "304 and 0304 are one piece");
    let p304 = review.pieces.iter().find(|p| p.number.value.as_deref() == Some("0304")).unwrap();
    assert_eq!((p304.title.level, p304.level), (Level::Red, Level::Red), "the title must be typed");
    // No key is assumed; in an English batch "B" is ambiguous and left empty.
    let k2 = part(p304, "Klarinette 2");
    assert_eq!((k2.key.value.clone(), k2.voices.values.clone()), (None, vec!["2".to_string()]));
    let kb = part(p304, "Klarinette in B");
    assert_eq!((kb.key.value.clone(), kb.key.level), (None, Level::Yellow));
    // The same instrument named twice ("Klarinette Klar") is listed once.
    let k3 = part(p304, "Klarinette Klar 3");
    assert_eq!(
        (k3.instruments.iter().map(|i| i.instrument_id).collect::<Vec<_>>(), review_name(k3).as_str(), k3.level),
        (vec![Some(klarinette.id)], "Klarinette 3", Level::Green)
    );

    // Bulk edit and a typed title; the part's key is chosen by the librarian.
    let ids: Vec<Id> = review.pieces.iter().map(|p| p.id).collect();
    let edited: Vec<ReviewPiece> =
        s.ok(Method::PATCH, "/api/import/items", admin, json!({"ids": ids, "piece": {"genre": "Marsch"}})).await;
    assert!(edited.iter().all(|p| p.genre.value.as_deref() == Some("Marsch")));
    s.ok::<Vec<ReviewPiece>>(
        Method::PATCH,
        "/api/import/items",
        admin,
        json!({"ids": [p304.id], "piece": {"title": "Alte Kameraden"}}),
    )
    .await;
    let bad = s.json(Method::PATCH, "/api/import/items", admin, json!({"ids": [kb.id], "part": {"key": "B"}})).await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST, "ambiguous key refused");
    s.ok::<Vec<ReviewPiece>>(
        Method::PATCH,
        "/api/import/items",
        admin,
        json!({"ids": [kb.id], "part": {"key": "in Bb"}}),
    )
    .await;
    // The voices of a part are a list; "&" typed in one entry separates voices.
    let many: Vec<String> = (1..=11).map(|i| i.to_string()).collect();
    let bad =
        s.json(Method::PATCH, "/api/import/items", admin, json!({"ids": [kb.id], "part": {"voices": many}})).await;
    assert_eq!(bad.error(), (StatusCode::BAD_REQUEST, ErrorCode::TooManyVoices { max: 10 }), "at most 10 voices");
    let edited: Vec<ReviewPiece> = s
        .ok(Method::PATCH, "/api/import/items", admin, json!({"ids": [kb.id], "part": {"voices": ["1 & 2", "1"]}}))
        .await;
    let kb_now = part(&edited[0], "Klarinette in B");
    assert_eq!(
        (kb_now.voices.values.clone(), kb_now.voices.source),
        (vec!["1".to_string(), "2".to_string()], Some(Source::Librarian))
    );

    // Several instruments and voices for one part (a-import-instruments).
    let trumpet = lib.trumpet_part.instrument_ids[0];
    let set = |instruments: serde_json::Value| json!({"ids": [k2.id], "part": {"instruments": instruments}});
    let bad = s.json(Method::PATCH, "/api/import/items", admin, set(json!([trumpet, 9999]))).await;
    assert_eq!(bad.error(), (StatusCode::BAD_REQUEST, ErrorCode::UnknownInstruments), "unknown instrument refused");
    let bad = s.json(Method::PATCH, "/api/import/items", admin, set(json!((0..21).collect::<Vec<_>>()))).await;
    assert_eq!(
        bad.error(),
        (StatusCode::BAD_REQUEST, ErrorCode::TooManyInstruments { max: 20 }),
        "at most 20 instruments"
    );
    let edited: Vec<ReviewPiece> = s
        .ok(
            Method::PATCH,
            "/api/import/items",
            admin,
            json!({"ids": [k2.id], "part": {"instruments": [klarinette.id, trumpet], "voices": ["2", "3"]}}),
        )
        .await;
    let k2_now = part(&edited[0], "Klarinette 2");
    assert_eq!(
        k2_now.instruments.iter().map(|i| (i.instrument_id, i.source, i.level)).collect::<Vec<_>>(),
        [
            (Some(klarinette.id), Some(Source::Librarian), Level::Green),
            (Some(trumpet), Some(Source::Librarian), Level::Green)
        ]
    );
    assert_eq!((review_name(k2_now).as_str(), k2_now.level), ("Klarinette / Trumpet 2 & 3", Level::Green));

    // German: the same names are read again; changes to parts stay (by path).
    let changed: ImportBatch =
        s.ok(Method::PUT, &format!("/api/import/batches/{}", batch.id), admin, json!({"language": "de"})).await;
    assert_eq!(changed.language, Language::De);
    let review: ImportReview = s.get(&format!("/api/import/batches/{}", batch.id), admin).await.json();
    let p304 = review.pieces.iter().find(|p| p.number.value.as_deref() == Some("0304")).unwrap();
    assert_eq!(p304.title.value.as_deref(), Some("Alte Kameraden"), "piece changes kept (same group)");
    assert_eq!(part(p304, "Klarinette in B").key.source, Some(Source::Librarian));
    let done: CommittedImport = s
        .ok(Method::POST, &format!("/api/import/batches/{}/commit", batch.id), admin, json!({"piece_ids": [p304.id]}))
        .await;
    assert_eq!(done.committed, [p304.id]);
    let found: Vec<Score> = s.get("/api/scores?q=alte", admin).await.json();
    assert_eq!(found[0].archive_number.as_deref(), Some("0304"), "the first path (in order) gives the number");
    assert_eq!(names(&found[0].parts), ["Klarinette / Trumpet 2 & 3", "Klarinette 3", "Klarinette in B♭ 1 & 2"]);
    let found_parts = sorted(&found[0].parts);
    assert_eq!(found_parts[2].voices, ["1", "2"], "the commit fills the voice list");
    assert_eq!(found_parts[1].instrument_ids, [klarinette.id], "an instrument named twice is committed once");
    let mut both = vec![klarinette.id, trumpet];
    both.sort_unstable();
    assert_eq!(
        (found_parts[0].instrument_ids.clone(), found_parts[0].voices.clone()),
        (both, vec!["2".to_string(), "3".to_string()]),
        "the commit creates the part with both instruments and both voices"
    );

    // Cross-check with the system detection: a "Partitur" whose systems have one staff.
    s.ok::<Instrument>(Method::POST, "/api/instruments", admin, json!({"name": "Partitur"})).await;
    let score_pdf = staff_pdf(1, 8);
    let more: Vec<(&str, &[u8])> = vec![("29 Partitur.pdf", &score_pdf)];
    let todo = register(&s, admin, batch.id, &more).await;
    upload_all(&s, admin, &todo, &more).await;
    while musicus_server::layout::worker::run_once(&s.state).await.unwrap() {}
    let review: ImportReview = s.get(&format!("/api/import/batches/{}", batch.id), admin).await.json();
    let p29 = review.pieces.iter().find(|p| p.number.value.as_deref() == Some("29")).unwrap();
    let full = part(p29, "Partitur");
    assert_eq!((full.file.staves, full.level), (Some(1), Level::Yellow));
    assert!(full.notes.contains(&Note::ScoreWithOneStaff));

    // A second import of the same folder skips what is already in the library.
    let again: ImportBatch = s
        .ok(
            Method::POST,
            "/api/import/batches",
            admin,
            json!({"name": "Scans", "pattern": preset("flat-number").unwrap().pattern, "language": "de"}),
        )
        .await;
    let todo = register(&s, admin, again.id, &files).await;
    assert_eq!(
        (todo.files.duplicate, todo.upload.len()),
        (3, 0),
        "28 is known content but not in the library: no upload"
    );
    let review: ImportReview = s.get(&format!("/api/import/batches/{}", again.id), admin).await.json();
    assert_eq!(review.pieces.len(), 1);
}
