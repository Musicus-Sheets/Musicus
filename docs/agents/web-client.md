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
| `/parts/:id/edit` | `pages::score_edit::EditPart` | Shell | edit one part (L); link `Edit systems` next to `Open` |
| `/parts/:id/systems` | `pages::layout_edit::LayoutEditor` | Shell | system editor of the current file (L, R-READ-016; `layout.md`) |
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
| `render_crop(url, canvas_id, Crop) -> Option<(css_w, css_h)>` | `renderCrop` | system view: renders only the crop (pdf.js viewport `offsetX/Y`, scale = css width / crop width in pt) at devicePixelRatio into a cached off-screen canvas (LRU 24, key url\|page\|crop\|round(css w)\|dpr), copies it into the canvas; newest request per canvas wins (else null) |
| `prerender_crop(url, Crop)` | `prerenderCrop` | fills the crop cache (next view) |
| `retain_only(urls)` | `retainOnly` | drops other bytes/docs (destroys docs) and their crops |
| `set_pending(set, page, rev, bytes)` / `clear_pending` | `setPending`/`clearPending` | `pendingPages` Map; copies bytes (a-wasm-view-copy); on `pagehide` sends each with `fetch(PUT, keepalive, If-Match, x-musicus-csrf)` |
| `keep_awake(on)` | `keepAwake` | screen wake lock; `document.body.dataset.keepAwake` drives re-acquire on `visibilitychange` |
| `toggle_fullscreen()`, `copy_text()` | | |

pdf.js URLs: `/vendor/pdfjs/pdf.min.mjs` + worker `/vendor/pdfjs/pdf.worker.min.mjs` (legacy build, d-pdf-rendering); `getDocument({data, wasmUrl: '/vendor/pdfjs/wasm/'})` for JBIG2/JPEG 2000 scans (a-pdfjs-wasm).

## System layouts (`layouts.rs`)

`thread_local` `HashMap<sha256, FileLayout>` (memory only, R-SEC-001): `load(sha)` returns a cached done/failed layout or
fetches `GET /api/files/{sha}/layout` (pending is not cached); `retain_only(shas)`. `ProgrammeView` loads all layouts
of the programme after `bridge::retain_only` (R-READ-019).

## Ink drawing (`ink.rs`)

`PageBox {width, height, ox, oy, vw, vh}` (CSS px of the displayed page; crop offset and visible size; `PageBox::page(w,h)` = whole page, `PageBox::crop(w,h,x0,y0,x1,y1)`): `canvas_to_page(x,y)` adds the offset, `to_page(x,y)` (page CSS px) → u16 coords, `to_css((u16,u16))`, `len_css(units)`. `prepare(canvas_id, box)` sizes canvas to vw×vh × DPR, clears, sets transform (dpr, translate −offset), so layers are drawn in page CSS px. `draw_layer`: highlighters (alpha 0.35) first, then pens (per-segment width by pressure: `pen_width_css` = len × (0.55 + 0.9·p), p=0 ⇒ 0.5), stamps (glyph, serif), texts. `hit(box, layer, point, radius)` → ids within radius (strokes by segment distance, stamps by centre, text by bounding box). `new_id()` = random u64 from `Math.random`.

## Pages

Management happens in the normal views (d-inline-management): the same components render for every role and add
controls when `ui::can_manage()` / `ui::is_admin()`; members get exactly the member pages.

- `library.rs`: `GET /api/scores[?q=]` (150 ms debounce via `gloo_timers`), `GET /api/instruments` for names; `ScoreCard` (id `score-<id>`; L: `Edit` link + warning tags "no parts yet" / "n parts without PDF") + `PartTile` (L: wrapped in `.part-cell` with an `Edit` link under the `PartLink` tile; tile = thumbnail if `has_thumbnail`); `part_route(part, programme)`. `FOCUS_SCORE: GlobalSignal<Option<i64>>` (memory only) is set by the editors before returning to `/`; `Library` takes it once on mount, highlights that card (`.score.focus`) and scrolls it into view via `document::eval`.
- `programmes.rs`: list (L: `+ New programme`); `ProgrammeView` (L: `Edit`) loads programme, `programme_urls` (unique file URLs in concert order), `bridge::retain_only(urls)`, prefetch chunks of 3, progress `loaded/total/failed` (R-PROG-003, R-PROG-004).
- `forms.rs` (shared by the editors): `ScoreDraft` (form state, `to_input` trims/empties → `None`; `tags: Vec<String>`) + `ScoreFields` (tags edited by `TagInput` through `draft.map_mut(|d| &d.tags, |d| &mut d.tags)`, a `WriteSignal`, so every chip change is in the draft synchronously, before a submit handler reads it); `InstrumentPicker` (assignment picker over instruments; without `onsave` it only sets the form's `selected`, with `onsave` the caller persists; no instruments ⇒ link to the Instruments page); `upload_pdf(part, FileData)` (`read_bytes()` → `api::upload`); `PartEditor` (heading = live name; custom name/voice/key + current instruments → `PUT /api/parts/{id}` via `Save part`; instrument changes are saved at once with `PUT /api/parts/{id}` carrying the `saved` (last stored) custom name/voice/key/notes, restored on error, `Saved` for 2.5 s; `notes` are kept, no longer sent as `None`; upload/replace PDF; version list from `GET /api/parts/{id}/versions` in a `<details>`, shown when >1 version or `versions_open`; delete with confirm); `PartFields` + `preview_name` (key `<select>` over `musicus_api::naming::KEYS`, first option "— (concert pitch)" = none, a stored unknown key as "Other: …"; voice free text; "Custom name (optional)" whose placeholder is the derived name; a-part-names); `NewPartForm` (instruments, then key/voice/custom name, live "Shown as: …"; creates the part, then uploads the chosen PDF in a `spawn`ed task so the next part can be typed meanwhile; reports `(Part, Upload)` via `onstate`; `Upload = NoFile | Running(name) | Done(FileVersion) | Failed(msg)`); `AddedParts` (rows with upload state).
- Part names: every view shows `Part.display_name` from the server (library tiles `.part-label`, `AddedParts`, `EditPart` header, reader title, programme views); `AddedParts` adds the instrument names only when a custom name hides them.
- `score_edit.rs`: `NewScore` (step 1 `ScoreFields` → POST score (PUT when going back and forth); step 2 `AddedParts` + `NewPartForm`; `Finish` disabled while uploads run → `FOCUS_SCORE` + `/`; step indicator `Steps`), `EditScore` (details save, `PartEditor` per part, `NewPartForm`, delete piece with confirm "everyone's annotations are deleted"), `EditPart` (loads part + score; `PartEditor` with versions open; delete → `/scores/:id/edit`).
- `programme_edit.rs`: `ProgrammeEditor{id: Option}` behind `NewProgramme`/`EditProgramme`: title/date/notes, ordered items (↑ ↓ ×; `+ Add pieces` = `Picker` over all scores, nothing excluded so encores work, entries already present marked "already in the programme", appended in tick order), save = POST or PUT + `PUT …/items` (a `created` signal prevents a second POST on retry) → programme view; delete → `/programmes`.
- `instruments.rs`: list grouped by family (`by_family`, "Other" last); L: add form (family `SuggestInput` over the known families), `InstrumentRow` inline edit (`PUT`) and delete (409 ⇒ "cannot be deleted because parts still use it").
- `groups.rs`: `Groups` + `GroupEditor` (members only if admin; instrument grants; score grants; create/delete group only admin). Each list is a `SavedList` → `AssignList`; every change (picker closed with new entries, × pressed) is sent at once as the full list (`PUT /api/groups/{id}/members|instrument-grants|score-grants`), controls disabled while it runs, `Saving…` → `Saved` (2.5 s) or error + previous list restored; no Save button.
- `users.rs`: `Users` + `UserRow` (create → invite link shown + copy; role select; disable/enable; new invite).
- `login.rs`, `account.rs`: forms over auth endpoints.

## Assignment picker (`picker.rs`, a-assign-picker)

Replaces checkbox-per-entry lists. `AssignList {options, assigned, onadd, onremove, add_label, empty, placeholder, busy, status}`: assigned entries as `.chip`s (label + muted detail, `×` button 44 px, `aria-label "Remove <label>"`), sorted by folded label; `empty` text when none; then `Picker` with the unassigned entries and the `Status`. Persisting is the caller's job.

`Picker {options, label, placeholder, onapply, disabled, none_left}`: toggle button (`aria-expanded`) → transparent fixed `.picker-backdrop` (z 30) + `.picker-panel` (z 31, absolute under the button; ≤ 600 px: fixed sheet under the top bar, dimmed backdrop). Panel: search `input[role=combobox]` focused on mount (`MountedData::set_focus`), listbox (`aria-multiselectable`) of `div[role=option][tabindex=-1][aria-selected]` with a check box glyph, footer `n selected` + `Add n` / `Close`. Filter: query folded and split at whitespace, every term must occur in the entry's folded label + detail + extra text (`fold`: NFD via `js_sys::JsString::normalize`, combining marks dropped, ß→ss etc.). Ticked ids kept in tick order (also when filtered out). Closing — backdrop click/tap, Escape anywhere in the panel, `Add n`/`Close` — calls `onapply(ticked)` if any, clears query; keyboard closes return focus to the toggle. There is no cancel: unticking before closing or × afterwards undoes. Keys: search field ↓ → first option, Enter → toggles the first match (prevent_default, so the surrounding form is never submitted); option ↓/↑ move focus (↑ on the first → search field), Enter/Space toggle, a printable key appends to the query and refocuses the search. The backdrop swallows the outside click (it only closes): an editor cannot unmount mid-save and a click on Save/another group is not half-applied.

Users: group members (label display name, detail username), instrument grants and part instruments (name, family), score grants (title, composer; subtitle/arranger searched), programme pieces (title, composer; options not reduced).

## Tag entry (`tag_input.rs`, a-tag-input)

`TagInput {tags: WriteSignal<Vec<String>>, labelledby, placeholder}` inside `div.tag-input[role=group]` styled like an input. Children: one `span.tag.tag-chip` per tag (text `.tag-text`, `button.tag-remove` "×", `aria-label "Remove tag <t>"`, 44 px hit area via `::after`) and one `input.tag-entry` at `cursor` (index among chips; `END = usize::MAX` = after the last chip, so tags loaded later keep the input at the end). Keyed list (`chip-<lowercase>`, `entry`), so moving the input is a DOM move; it is refocused with `requestAnimationFrame`.

- Commit (`add`): split at commas, trim, skip empty, skip duplicates ignoring case (that chip gets `.flash` for 0.9 s), insert at the input's place. Triggers: keydown `,` (prevented), Enter (always prevented: never submits the form), Tab (not prevented), blur (so a tag typed without comma is kept when the user clicks Save/Next), and the input event when the value contains a comma (Android keyboards send `Unidentified` keydowns): text before the last comma becomes chips, the rest stays typed; if the value grew by more than one character at once (paste, keyboard suggestion) everything becomes chips.
- Caret moves: ←/→/Home/End only when the input is empty; Backspace with the caret at 0 removes the chip before, Delete with the caret at the end the chip after (caret read synchronously through `web_sys::HtmlInputElement::selection_start/end`). Clicking a chip puts the input after it; clicking free space puts it at the end. Chip removal and placement look chips up by text, because a blur just before the click may already have inserted a chip.

## Suggestions (`suggest.rs`, a-tag-suggest)

`suggestions(all, query, exclude)`: `picker::fold` (case, diacritics); rank 0 = starts with the query, 1 = a word starts with it, 2 = contains it; stable within a rank (server order), max 8; entries in `exclude` (ignoring case) dropped; empty query ⇒ none. `on_key(e, items, open, active)`: ↓/↑ cycle `active` (nothing active until pressed, so Enter keeps its normal meaning), Enter/Tab with an active item take it (prevented), Escape closes (prevented, propagation stopped) ⇒ `(handled, taken)`. `SuggestList`: `ul.suggest[role=listbox]`, items `li[role=option][id=<list>-<i>]`, `onmousedown` prevented (the input keeps focus, so the tag input's blur-commit does not fire), click takes. `SuggestInput {value, options, placeholder, label, required}`: input `role=combobox` + list in `.suggest-wrap`; open on input, closed on blur/take/Escape; hides a single suggestion equal to the value.

`TagInput` (prop `suggestions`): the input sits in `span.tag-entry-wrap` (positioned), the list hangs under it wherever it is among the chips; options = known tags minus chosen ones; a taken suggestion goes through `add` like typed text; comma/Enter without an active suggestion commit the typed text. `ScoreFields` loads the known tags with `GET /api/tags`.

## Reader (`pages/viewer.rs`) — most complex unit

Page view or system view (F-READ-003): the exact view algorithm, signals/memos (`settings`, `layout`, `items`,
`views`, `view_idx`, `crops`, `anchor`), rendering of one contiguous crop of the original page per page segment, ink per segment (`Target`), swipe and the reader settings
switch are described in `layout.md` § Client. Bar: `☰ Systems` toggle (`aria-pressed`), in system view a
`select.staff-size` (`aria-label "Staff size"`, S/M/L/XL); changes update `Session.user.reader` and PUT
`/api/auth/reader-settings`. Banner `.banner.info` while the layout is pending/failed. `.system-view[data-view]`.

Load: `ViewerPage` → `load(id, pid)`: `GET /api/parts/{id}`, `GET /api/scores/{score_id}`, `GET /api/parts/{id}/annotation-sets`, optional `GET /api/programmes/{pid}` → `Reader`.

Signals: `sets`, `active` (set id), `page`, `page_total`, `stage` (size from `onresize`), `page_box`, `mode` (Read/Draw), `tool`, colours/widths, `show_ink`, `status` (SaveStatus), `error`, `menu`, `version` (redraw trigger). Memo `url`: file of active set (may be an old version) else current file.

Non-signal state (d-reader-store): `store: Rc<RefCell<Store>>`, `Store.pages: HashMap<(set_id, page), PageState>`; `PageState {layer, revision, base_ids, loaded, generation, saved_generation, saving, undo, redo}`; `live: Rc<RefCell<Live>>` (current stroke, pointer id, `pen_seen`).

Effects: url → `page_count`; (url, stage, page) → `render_page` into `page-canvas` (box = stage − 8 px) → `page_box`; (active, page) → `load_page`; (version, page_box, show_ink, active, page) → `ink::draw_layer` into `ink-canvas`. On mount: `keep_awake(true)`, prefetch next piece. On drop: flush dirty pages with `spawn_forever(save_page…)`, `keep_awake(false)`; keyboard listener removed.

`load_page(store, key)`: GET page; decode; if local objects were drawn before load finished ⇒ `merge(∅, local, server)`; set `revision`, `base_ids`, `loaded`.

`save_page(store, key)` (≤5 iterations, touches no signals): skip if clean/saving/not loaded; PUT with `revision`; Saved ⇒ `revision`, `base_ids = ids(sent layer)`, `saved_generation = generation_sent`, `sync_pending`, stop if no newer changes; Conflict ⇒ GET server, `layer = merge(base_ids, layer, server)`, `base_ids = ids(server)`, `revision = server rev`, `generation += 1`, `sync_pending`, retry.

`change(key, f)`: apply f to PageState → push undo action, clear redo, `generation += 1`, `sync_pending`, bump `version`, schedule `save(key, delay=true)` (1500 ms; skipped if generation changed meanwhile). `add_object`: if no active set ⇒ POST new set (name = programme title or "My annotations", linked programme) then add. `erase_at` removes all hit ids (one `Action::Remove`). `undo_redo` inverts `Add`/`Remove` actions.

Input on `ink-canvas` / `crop-ink-{k}` (pointer events active only in Draw mode via CSS class; each handler gets a `Target {page, bx, canvas}`): pen/highlighter collect points (page CSS px via `canvas_to_page` + pressure; mouse ⇒ pressure 0), paint segments live (a-live-ink), on up convert to page coords (dedupe consecutive equal points, cap 20 000) ⇒ `Stroke::from_points`; touch ignored after a pen was seen (R-ANN-004); eraser on down/move; stamp on down; text via `prompt`.

Navigation `turn(delta)`: flush dirty pages immediately; in the system view by view (see `layout.md`); else `page+delta < 0` ⇒ previous piece route; `≥ page_total` ⇒ next piece route; else set page. Tap zones in Read mode: x < 1/3 width ⇒ −1, > 2/3 ⇒ +1 (ignored for 400 ms after a swipe). Swipe (pointer down/up on the stage, Read mode, |dx| ≥ 60 px and > 2|dy|): left ⇒ +1, right ⇒ −1 (R-READ-009). Keys via `document::eval` listener (stored in `window.__musicusKeys`, ignores INPUT/TEXTAREA/SELECT): ArrowLeft/ArrowUp/PageUp ⇒ −1; ArrowRight/ArrowDown/PageDown/Space ⇒ +1 (R-READ-002). `neighbours(programme, part)`: previous/next item with a part that has a file, preferring shared instrument (R-READ-005).

Sets menu: radio list (shows "default" and outdated `v<n>` tags), New, Duplicate (`copy_from`), Rename, Make default, Use for this programme, Delete; banner offers `migrate` when `set.file_version != part.file.version` (R-ANN-016). `choose_set`: programme-linked set ⇒ default ⇒ first (R-ANN-009).
