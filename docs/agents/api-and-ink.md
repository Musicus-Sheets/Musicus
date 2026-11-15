# musicus-api crate: DTOs and ink format

Crate `crates/musicus-api`: deps serde + postcard only, no I/O, compiles for native and wasm32 (INV-14, d-shared-types).

## DTO conventions (`dto.rs`)

- `Id = i64`. Timestamps i64 Unix seconds. Dates `Option<String>` `YYYY-MM-DD`.
- `Role` serialises lowercase; ordering Member < Librarian < Admin; `can_manage_library()` = librarian|admin; `is_admin()`.
- Error body for every non-2xx (except 409 page conflict and 304/204): `ApiError {error: code, message}`.
- 409 on page save: body `PageConflict {current_revision}` (not ApiError).
- Whole-list replacement uses `IdList {ids}` with PUT (idempotent).
- `UpdateUser`, `UpdateAnnotationSet`: absent fields = unchanged. `UpdateAnnotationSet.programme_id: Option<Option<Id>>` via `double_option`: absent = unchanged, `null` = unlink, number = link (unit test `programme_link_distinguishes_absent_and_null`).
- `Score.parts` contains only parts readable by the requester; `Part.file` = current `FileVersion` or None.
- `AnnotationSet.file_sha256/file_version` = the version the set is bound to (compare with `Part.file.version` to detect outdated sets); `pages` = saved pages with revisions.
- Constants: `CSRF_HEADER = "x-musicus-csrf"`, `SESSION_COOKIE = "musicus_session"`.

Adding an endpoint: request/response structs here → handler in server → client helper call. Keep field names stable (future app).

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
