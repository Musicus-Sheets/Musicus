# musicus-api crate: DTOs and ink format

Crate `crates/musicus-api`: deps serde + postcard only, no I/O, compiles for native and wasm32 (INV-14, d-shared-types).

## DTO conventions (`dto.rs`)

- `Id = i64`. Timestamps i64 Unix seconds. Dates `Option<String>` `YYYY-MM-DD`.
- `Role` serialises lowercase; ordering Member < Librarian < Admin; `can_manage_library()` = librarian|admin; `is_admin()`.
- Error body for every non-2xx (except 409 page conflict and 304/204): `ApiError {error: code, message}`.
- 409 on page save: body `PageConflict {current_revision}` (not ApiError).
- Whole-list replacement uses `IdList {ids}` with PUT (idempotent).
- `UpdateUser`, `UpdateAnnotationSet`: absent fields = unchanged. `UpdateAnnotationSet.programme_id: Option<Option<Id>>` via `double_option`: absent = unchanged, `null` = unlink, number = link (unit test `programme_link_distinguishes_absent_and_null`).
- `Score.parts` contains only parts readable by the requester, sorted by `naming::natural_cmp` of `display_name`; `Part.file` = current `FileVersion` or None.
- `Part {label: Option (custom name), display_name (always set: custom or derived), voice: Option<String>, transposition: Option<String> (key code or legacy text)}`. `PartInput.label`/`transposition`: blank ⇒ None (`blank_is_none`); `voice` accepts a JSON string or number (`text_or_number`), blank ⇒ None (test `part_input_accepts_blank_label_and_numeric_voice`).
- `AnnotationSet.file_sha256/file_version` = the version the set is bound to (compare with `Part.file.version` to detect outdated sets); `pages` = saved pages with revisions.
- Constants: `CSRF_HEADER = "x-musicus-csrf"`, `SESSION_COOKIE = "musicus_session"`.

- `CurrentUser.reader: layout::ReaderSettings` (`#[serde(default)]`); system layout types, reading sequence and view
  planner live in `layout.rs` (described in `layout.md`).

Adding an endpoint: request/response structs here → handler in server → client helper call. Keep field names stable (future app).

## Part names and keys (`naming.rs`, a-part-names, R-LIB-010, R-LIB-011)

- `part_name(custom, instrument_names, key, voice)`: trimmed custom name if non-empty, else `derived_name`: instrument names sorted by `natural_cmp`, case-insensitively deduped, joined `" / "` (none ⇒ `Part`), `+ " in " + key_name(key)`, `+ " " + voice`; nothing at all ⇒ `Unnamed part`. Examples: `Trumpet in B♭ 1`, `Flute / Piccolo`, `Horn in F 3a`, `Part 2`.
- `KEYS: &[Key {code, name, option}]` in selection order: Bb B♭, Eb E♭, F, C, A, D, G, E, Ab A♭, Db D♭, Gb G♭, H B♮ (option "B♮ (H)"). None = concert pitch. `key(code)`, `key_name(code)` (unknown ⇒ unchanged).
- `parse_key(text)`: unambiguous spellings ⇒ code (mirrors migration 0002); "B" ⇒ None (B♭ in German). `normalize_key` = exact code or `parse_key` (server input).
- `natural_cmp`: case-insensitive, digit runs by value.
- The server computes `display_name` (it has the instrument names); the client calls `part_name` only for the live preview in forms.

## Ink format (`ink.rs`) — d-ink-format, R-ANN-005..007

```
PageLayer { objects: Vec<InkObject> }
InkObject = Stroke { id: u64, tool: Pen|Highlighter, color: u32 RGBA, width: u16, start: [u16;2], deltas: Vec<[i32;2]>, pressure: Vec<u8> }
          | Stamp  { id, kind: StampKind(8 kinds), at: [u16;2], size: u16, color }
          | TextNote { id, at: [u16;2] (baseline left), size, color, text }
```
- Coordinates: 0..=65535 (`COORD_MAX`) relative to page box, origin top-left. Lengths in 1/65535 of page width.
- `pressure.len() == deltas.len() + 1`; pressure 0 = unknown (mouse) → client draws medium width.
- Use `Stroke::from_points(id, tool, color, width, &[(x,y,p)])` and `stroke.points()` (clamps); do not build deltas by hand.
- Wire/storage bytes: `[FORMAT_VERSION=1] ++ postcard(PageLayer)`. Server stores `zstd::encode_all(bytes, 3)`.
- `decode` rejects: empty, other version, > `MAX_ENCODED_BYTES` 4 MiB, postcard error, > `MAX_OBJECTS_PER_PAGE` 20 000, stroke > `MAX_POINTS_PER_STROKE` 20 000 points, text > `MAX_TEXT_CHARS` 1000, duplicate ids, pressure length mismatch (R-ANN-006).
- `StampKind::glyph()` gives the drawn character (client uses it; changing glyphs changes the look of stored data).

## Merge (`ink::merge(base_ids, local, server)`) — d-optimistic, R-ANN-012

Result = local objects minus those in `base_ids` but absent on server (deleted remotely) + server objects absent locally and not in `base_ids` (added remotely). Order: local first, then remote additions. Correct only because objects are immutable (INV-7). Tests: `merge_keeps_both_sides_additions_and_deletions`, `merge_with_empty_base_is_a_union`.

Changing the format: bump `FORMAT_VERSION`, keep v1 decoding (stored data), add tests, update server route body limit if `MAX_ENCODED_BYTES` changes, update this file.
