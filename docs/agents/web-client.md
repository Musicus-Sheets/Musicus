# Web client (`crates/musicus-web`)

Dioxus 0.7.10, features `web` + `router`; compiled to wasm32 by `dx`; assets `assets/main.css` (via `asset!`), `public/vendor/pdfjs/*` (fetched, gitignored), `js/bridge.js` (wasm-bindgen snippet). Talks only to `/api` (same origin). Persists nothing (INV-10).

## Routes (`main.rs::Route`)

| Path | Component | Layout | Notes |
| --- | --- | --- | --- |
| `/login` | `pages::login::Login` | none | redirects to `/` once `Session.user` set |
| `/invite/:token` | `pages::login::Invite` | none | GET info, POST password; sets `Session.user` |
| `/parts/:id` | `pages::viewer::Viewer` | none (full screen) | reader without programme |
| `/programmes/:pid/parts/:id` | `pages::viewer::ProgrammeViewer` | none | reader with prev/next piece |
| `/` | `pages::library::Library` | `ui::Shell` | librarian/admin: `+ Add piece`, `Edit` per piece and part |
| `/scores/new` | `pages::score_edit::NewScore` | Shell | guided flow (L) |
| `/scores/:id/edit` | `pages::score_edit::EditScore` | Shell | edit piece (L) |
| `/parts/:id/edit` | `pages::score_edit::EditPart` | Shell | edit one part (L) |
| `/programmes`, `/programmes/:id` | `pages::programmes::{Programmes, ProgrammeView}` | Shell | ProgrammeView prefetches; L: `+ New programme`, `Edit` |
| `/programmes/new`, `/programmes/:id/edit` | `pages::programme_edit::{NewProgramme, EditProgramme}` | Shell | (L) |
| `/instruments` | `pages::instruments::Instruments` | Shell | everyone reads; L adds/edits/deletes |
| `/groups` | `pages::groups::Groups` | Shell | (L); members of a group only editable by admins |
| `/users` | `pages::users::Users` | Shell | (A) |
| `/account` | `pages::account::Account` | Shell | |
| `/manage`, `/manage/:..segments` | redirect → `/` | — | old bookmarks of the removed Manage area |
| `/:..segments` | `NotFound` | none | |

(L) = page shown to librarians/admins only, (A) = admins only; others get `ui::NoAccess`. These are UI conveniences: the
server checks every request (d-inline-management). Static segments (`new`) win over `:id` in the router.

Navigation (`ui::Shell`): `Library`, `Programmes`, `Instruments` for all; `Groups` for librarian/admin; `Users` for admin;
then the display name (account) and `Log out`. Links use `active_class: "active"`.

Server serves `index.html` for any non-`/api` path, so deep links work.

## Session (`main.rs`)

`Session {user: Signal<Option<CurrentUser>>, checked: Signal<bool>}` context. Role helpers (not hooks, usable anywhere in a component): `ui::current_role()`, `ui::can_manage()` (librarian/admin), `ui::is_admin()`. `App` calls `GET /api/auth/me` once. `ui::Shell` redirects to `/login` when `checked && user.is_none()`. Logout = `POST /api/auth/logout` then `user = None`. Any 401 elsewhere ⇒ `api::session_lost` ⇒ `location.href = "/login"` (a-unauthorized-reload).

## HTTP (`api.rs`)

`builder()` always adds `x-musicus-csrf: 1` (INV-12). Helpers: `get`, `send/post/put/patch` (JSON), `call` (no body, expects 2xx), `call_json`, `upload` (raw bytes + content type). Errors → `ApiErr {status, message}` (message from `ApiError`, capitalised; status 0 = network). Ink: `get_page(set, page)` → `PageBytes::Found{bytes, revision from ETag}` | `Missing` (204); `put_page(set, page, expected_rev, bytes)` → `SaveOutcome::Saved(rev)` | `Conflict` (409). URLs: `file_url(sha)`, `thumbnail_url(sha)`.

## Bridge (`bridge.rs` ↔ `js/bridge.js`)

| Rust | JS export | Behaviour |
| --- | --- | --- |
| `prefetch(url) -> impl Future` (eager, a-eager-prefetch) | `prefetch` | `bytes: Map<url, Promise<Uint8Array>>`; `fetch(url, {credentials: same-origin})`; failed promise removed |
| `is_cached(url)` | `isCached` | |
| `page_count(url)` | `pageCount` | opens doc via pdf.js (`docs` Map; `getDocument({data: bytes.slice()})` because pdf.js transfers the buffer to its worker) |
| `render_page(url, index, canvas_id, w, h) -> Option<(css_w, css_h)>` | `renderPage` | scale = fit box; render to off-screen canvas at devicePixelRatio; cancels older task for same canvas (`tasks` Map) → returns null if cancelled; copies into visible canvas, sets CSS size |
| `retain_only(urls)` | `retainOnly` | drops other bytes/docs (destroys docs) |
| `set_pending(set, page, rev, bytes)` / `clear_pending` | `setPending`/`clearPending` | `pendingPages` Map; copies bytes (a-wasm-view-copy); on `pagehide` sends each with `fetch(PUT, keepalive, If-Match, x-musicus-csrf)` |
| `keep_awake(on)` | `keepAwake` | screen wake lock; `document.body.dataset.keepAwake` drives re-acquire on `visibilitychange` |
| `toggle_fullscreen()`, `copy_text()` | | |

pdf.js URLs: `/vendor/pdfjs/pdf.min.mjs` + worker `/vendor/pdfjs/pdf.worker.min.mjs` (legacy build, d-pdf-rendering).

## Ink drawing (`ink.rs`)

`PageBox {width, height}` (CSS px of displayed page): `to_page(x,y)` → u16 coords, `to_css((u16,u16))`, `len_css(units)`. `prepare(canvas_id, box)` sizes canvas to box × DPR, sets transform, clears. `draw_layer`: highlighters (alpha 0.35) first, then pens (per-segment width by pressure: `pen_width_css` = len × (0.55 + 0.9·p), p=0 ⇒ 0.5), stamps (glyph, serif), texts. `hit(box, layer, point, radius)` → ids within radius (strokes by segment distance, stamps by centre, text by bounding box). `new_id()` = random u64 from `Math.random`.

## Pages

Management happens in the normal views (d-inline-management): the same components render for every role and add
controls when `ui::can_manage()` / `ui::is_admin()`; members get exactly the member pages.

- `library.rs`: `GET /api/scores[?q=]` (150 ms debounce via `gloo_timers`), `GET /api/instruments` for names; `ScoreCard` (id `score-<id>`; L: `Edit` link + warning tags "no parts yet" / "n parts without PDF") + `PartTile` (L: wrapped in `.part-cell` with an `Edit` link under the `PartLink` tile; tile = thumbnail if `has_thumbnail`); `part_route(part, programme)`. `FOCUS_SCORE: GlobalSignal<Option<i64>>` (memory only) is set by the editors before returning to `/`; `Library` takes it once on mount, highlights that card (`.score.focus`) and scrolls it into view via `document::eval`.
- `programmes.rs`: list (L: `+ New programme`); `ProgrammeView` (L: `Edit`) loads programme, `programme_urls` (unique file URLs in concert order), `bridge::retain_only(urls)`, prefetch chunks of 3, progress `loaded/total/failed` (R-PROG-003, R-PROG-004).
- `forms.rs` (shared by the editors): `ScoreDraft` (form state, `to_input` trims/empties → `None`, tags split at commas) + `ScoreFields`; `InstrumentChecks`; `upload_pdf(part, FileData)` (`read_bytes()` → `api::upload`); `PartEditor` (label/voice/key/instruments → `PUT /api/parts/{id}`; upload/replace PDF; version list from `GET /api/parts/{id}/versions` in a `<details>`, shown when >1 version or `versions_open`; delete with confirm); `NewPartForm` (creates the part, then uploads the chosen PDF in a `spawn`ed task so the next part can be typed meanwhile; reports `(Part, Upload)` via `onstate`; `Upload = NoFile | Running(name) | Done(FileVersion) | Failed(msg)`); `AddedParts` (rows with upload state).
- `score_edit.rs`: `NewScore` (step 1 `ScoreFields` → POST score (PUT when going back and forth); step 2 `AddedParts` + `NewPartForm`; `Finish` disabled while uploads run → `FOCUS_SCORE` + `/`; step indicator `Steps`), `EditScore` (details save, `PartEditor` per part, `NewPartForm`, delete piece with confirm "everyone's annotations are deleted"), `EditPart` (loads part + score; `PartEditor` with versions open; delete → `/scores/:id/edit`).
- `programme_edit.rs`: `ProgrammeEditor{id: Option}` behind `NewProgramme`/`EditProgramme`: title/date/notes, ordered items (↑ ↓ ×, add via select), save = POST or PUT + `PUT …/items` (a `created` signal prevents a second POST on retry) → programme view; delete → `/programmes`.
- `instruments.rs`: list grouped by family (`by_family`, "Other" last); L: add form (family `datalist`), `InstrumentRow` inline edit (`PUT`) and delete (409 ⇒ "cannot be deleted because parts still use it").
- `groups.rs`: `Groups` + `GroupEditor` (members only if admin; instruments; score grants with filter; create/delete group only admin).
- `users.rs`: `Users` + `UserRow` (create → invite link shown + copy; role select; disable/enable; new invite).
- `login.rs`, `account.rs`: forms over auth endpoints.

## Reader (`pages/viewer.rs`) — most complex unit

Load: `ViewerPage` → `load(id, pid)`: `GET /api/parts/{id}`, `GET /api/scores/{score_id}`, `GET /api/parts/{id}/annotation-sets`, optional `GET /api/programmes/{pid}` → `Reader`.

Signals: `sets`, `active` (set id), `page`, `page_total`, `stage` (size from `onresize`), `page_box`, `mode` (Read/Draw), `tool`, colours/widths, `show_ink`, `status` (SaveStatus), `error`, `menu`, `version` (redraw trigger). Memo `url`: file of active set (may be an old version) else current file.

Non-signal state (d-reader-store): `store: Rc<RefCell<Store>>`, `Store.pages: HashMap<(set_id, page), PageState>`; `PageState {layer, revision, base_ids, loaded, generation, saved_generation, saving, undo, redo}`; `live: Rc<RefCell<Live>>` (current stroke, pointer id, `pen_seen`).

Effects: url → `page_count`; (url, stage, page) → `render_page` into `page-canvas` (box = stage − 8 px) → `page_box`; (active, page) → `load_page`; (version, page_box, show_ink, active, page) → `ink::draw_layer` into `ink-canvas`. On mount: `keep_awake(true)`, prefetch next piece. On drop: flush dirty pages with `spawn_forever(save_page…)`, `keep_awake(false)`; keyboard listener removed.

`load_page(store, key)`: GET page; decode; if local objects were drawn before load finished ⇒ `merge(∅, local, server)`; set `revision`, `base_ids`, `loaded`.

`save_page(store, key)` (≤5 iterations, touches no signals): skip if clean/saving/not loaded; PUT with `revision`; Saved ⇒ `revision`, `base_ids = ids(sent layer)`, `saved_generation = generation_sent`, `sync_pending`, stop if no newer changes; Conflict ⇒ GET server, `layer = merge(base_ids, layer, server)`, `base_ids = ids(server)`, `revision = server rev`, `generation += 1`, `sync_pending`, retry.

`change(key, f)`: apply f to PageState → push undo action, clear redo, `generation += 1`, `sync_pending`, bump `version`, schedule `save(key, delay=true)` (1500 ms; skipped if generation changed meanwhile). `add_object`: if no active set ⇒ POST new set (name = programme title or "My annotations", linked programme) then add. `erase_at` removes all hit ids (one `Action::Remove`). `undo_redo` inverts `Add`/`Remove` actions.

Input on `ink-canvas` (pointer events active only in Draw mode via CSS class): pen/highlighter collect points (CSS px + pressure; mouse ⇒ pressure 0), paint segments live (a-live-ink), on up convert to page coords (dedupe consecutive equal points, cap 20 000) ⇒ `Stroke::from_points`; touch ignored after a pen was seen (R-ANN-004); eraser on down/move; stamp on down; text via `prompt`.

Navigation `turn(delta)`: flush dirty pages immediately; `page+delta < 0` ⇒ previous piece route; `≥ page_total` ⇒ next piece route; else set page. Tap zones in Read mode: x < 1/3 width ⇒ −1, > 2/3 ⇒ +1. Keys via `document::eval` listener (stored in `window.__musicusKeys`, ignores INPUT/TEXTAREA/SELECT): ArrowLeft/ArrowUp/PageUp ⇒ −1; ArrowRight/ArrowDown/PageDown/Space ⇒ +1 (R-READ-002). `neighbours(programme, part)`: previous/next item with a part that has a file, preferring shared instrument (R-READ-005).

Sets menu: radio list (shows "default" and outdated `v<n>` tags), New, Duplicate (`copy_from`), Rename, Make default, Use for this programme, Delete; banner offers `migrate` when `set.file_version != part.file.version` (R-ANN-016). `choose_set`: programme-linked set ⇒ default ⇒ first (R-ANN-009).
