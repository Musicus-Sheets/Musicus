# musicus-api crate: DTOs and ink format

Crate `crates/musicus-api`: deps serde + postcard only, no I/O, compiles for native and wasm32 (INV-14, d-shared-types).

## DTO conventions (`dto.rs`)

- `Id = i64`. Timestamps i64 Unix seconds. Dates `Option<String>` `YYYY-MM-DD`.
- `Role` serialises lowercase; ordering Member < Librarian < Admin; `can_manage_library()` = librarian|admin; `is_admin()`.
- Error body for every non-2xx (except 409 page conflict and 304/204): `ApiError {error: code, message}`.
- 409 on page save: body `PageConflict {current_revision}` (not ApiError).
- Whole-list replacement uses `IdList {ids}` with PUT (idempotent).
- `UpdateUser`, `UpdateAnnotationSet`: absent fields = unchanged. `UpdateAnnotationSet.programme_id: Option<Option<Id>>` via `double_option`: absent = unchanged, `null` = unlink, number = link (unit test `programme_link_distinguishes_absent_and_null`).
- `ScoreInput.archive_number`: blank ⇒ None (`blank_is_none`); `Score.archive_number` (`#[serde(default)]`).
- `Score.parts` contains only parts readable by the requester, sorted by `naming::natural_cmp` of `display_name`; `Part.file` = current `FileVersion` or None.
- `Part {label: Option (custom name), display_name (always set: custom or derived), voices: Vec<String> (stored order, maybe empty), transposition: Option<String> (key code or legacy text)}`. `PartInput.label`/`transposition`: blank ⇒ None (`blank_is_none`); `voices` (missing/null ⇒ `[]`) is a list whose entries may be JSON strings or numbers, cleaned by `naming::clean_voices` during deserialisation (`voice_list`; test `part_input_accepts_blank_label_and_numeric_voices`). The old single `voice` field is gone (clean switch, nothing released; serde ignores it like any unknown field, a-part-voices).
- `AnnotationSet.file_sha256/file_version` = the version the set is bound to (compare with `Part.file.version` to detect outdated sets); `pages` = saved pages with revisions.
- Constants: `CSRF_HEADER = "x-musicus-csrf"`, `SESSION_COOKIE = "musicus_session"`.

- `CurrentUser.reader: layout::ReaderSettings` (`#[serde(default)]`); system layout types, reading sequence and view
  planner live in `layout.rs` (described in `layout.md`).

Adding an endpoint: request/response structs here → handler in server → client helper call. Keep field names stable (future app).

## Part names and keys (`naming.rs`, a-part-names, R-LIB-010, R-LIB-011)

- `part_name(custom, instrument_names, key, voices)`: trimmed custom name if non-empty, else `derived_name`: instrument names sorted by `natural_cmp`, case-insensitively deduped, joined `" / "` (none ⇒ `Part`), `+ " in " + key_name(key)`, `+ " " + voices joined with VOICE_SEPARATOR " & "` (stored order, blanks skipped); nothing at all ⇒ `Unnamed part`. Examples: `Trumpet in B♭ 1`, `Flute / Piccolo`, `Horn in F 3a`, `Klarinette 1 & 2`, `Flöte / Oboe 1 & 2`, `Trombone 1 & 2 & 3`, `Part 2`.
- `clean_voices(list)`: each entry split at `&` (the name separator), trimmed, blanks dropped, duplicates ignoring case dropped (first kept), order kept. Limits `MAX_VOICES` = 10, `MAX_VOICE_CHARS` = 20 (checked by the server, 400). `natural_cmp` on the shown names orders several voices by the first, then the rest (`Clarinet 1` < `Clarinet 1 & 2` < `Clarinet 2`).
- `KEYS: &[Key {code, name, option}]` in selection order: Bb B♭, Eb E♭, F, C, A, D, G, E, Ab A♭, Db D♭, Gb G♭, H B♮ (option "B♮ (H)"). None = concert pitch. `key(code)`, `key_name(code)` (unknown ⇒ unchanged).
- `parse_key(text)`: unambiguous spellings ⇒ code (mirrors migration 0002); "B" ⇒ None (B♭ in German). `normalize_key` = exact code or `parse_key` (server input).
- `natural_cmp`: case-insensitive, digit runs by value.
- Archive numbers (a-archive-number, R-LIB-015): `archive_key(n)` = letters/digits lower case, leading zeros of a leading number dropped ("0304"/"304" ⇒ "304", "A-12" ⇒ "a12"), for matching; `archive_search_text(n)` = forms indexed by the search ("0304 304", "A-12 A12").
- The server computes `display_name` (it has the instrument names); the client calls `part_name` only for the live preview in forms.

## Mass import (`import/`, a-import-shared, F-LIB-004)

Pure, used by the server (stores proposals, resolves on every read, commits) and by the client (live preview of a pattern on the selected file list). No DB access: the caller passes `review::Library {instruments, aliases, scores, groups}`.

- Paths are relative to the selected folder (`strip_common_root`); only `*.pdf` (`is_pdf_name`).
- `pattern::Pattern`: `{number}` (no whitespace, ≥1 digit, shortest), `{title}` `{composer}` `{arranger}` `{genre}` (longest), `{part}` (longest), `{*}` (ignored, shortest); `[…]` optional (expanded; the alternative with most groups present that matches wins); `/` folder separator, fields never contain `/`; literals case-insensitive; trailing `.pdf` dropped on both sides; values cleaned (`_` ⇒ space, spaces collapsed). Must contain `{part}` and `{title}` or `{number}`; ≤ 6 optional groups. `PRESETS`: folder (default), category, flat-title, flat-number. `detect(paths)` sorts presets by matched paths, then by part names with a dictionary instrument.
- `parts::parse_part(text, language)`: tokens (`tokenize`), dictionary longest match (≤3 words), keys via `in X`, `X flat` or a standalone key word, voices as a list `Voices {values, level, note}` (digits without leading zeros, `3a`, roman I–VIII, `Solo`; cleaned by `naming::clean_voices`, order as written; `+`, `&`, `/`, `,` (all tokenised to `+`), `und`, `and` directly between two voices ⇒ green, e.g. "1+2", "1/2", "1 & 2", "I+II" ⇒ ["1", "2"]; two voices without such a separator ("1 2", "1 in B 2") ⇒ yellow `SeveralVoices`, a-import-voices), fillers ignored (`Stimme`, `part`, `st`/`nd`…), prefix/longer-form guesses yellow; unknown words become a red unknown instrument when no instrument was found, else `rest` (yellow hint). Key: never assumed; `parse_import_key` maps unambiguous spellings in both languages, German letters (B, Es, As, Des, Ges, H) only for `Language::De`, else yellow `AmbiguousKey` without value; two different keys ⇒ red `TwoKeys`. `naming::parse_key` is unchanged.
- `review::propose` takes the paths in byte order (preview and server agree on the first candidate), groups by archive key or title+composer; field candidates (deduped) carry `Source::Pattern`; >1 candidate ⇒ red `Contradictory` until overridden.
- `review::review_piece`: field = override (source librarian) or candidates; red `TitleMissing` for a new piece without title; suggestion = existing score with same `archive_key`, else same `norm(title)` + `norm(composer)` ⇒ yellow `ConfirmTarget` until `Target` chosen; same title with other composer ⇒ yellow `SimilarTitle`; `Target::Existing` of a deleted score ⇒ red. Parts: instruments via `resolve_instrument` (stored alias green ⇒ instrument named exactly so green ⇒ dictionary spelling of the entry: green for full words, yellow for abbreviation/guess/ambiguous ⇒ red unknown) unless `PartOverrides.instruments` (a list of `InstrumentPick`: `Id` = instrument of the library, source librarian, green; `Word(alias)` = an instrument word of the file name kept and resolved like the detection, unknown stays red; unknown ids and words not in the name are dropped, an instrument twice is shown once; `Some([])` ⇒ red `NoInstrument`; at most `MAX_PART_INSTRUMENTS` = 20; JSON numbers/strings, `instrument_ids` accepted as alias; `ReviewPart::instrument_picks`/`ResolvedInstrument::pick` give the current list in this form, a-import-instruments); key override must pass `naming::normalize_key` (else red); voices = `PartOverrides.voices` (cleaned, source librarian, green) or the parsed list with its level/note (`VoicesField`); the name joins them with " & " (`naming::part_name`); file pending/failed ⇒ red; staves per system (`staves_per_system`, median) ⇒ yellow `ScoreWithOneStaff` (Partitur with ≤1) / `ManyStaves` (one non-score instrument, ≥4); readers = groups with an instrument grant of a resolved instrument (+ score grants of the target piece), `NoReaders` hint does not change the level. Piece level = max of fields, notes and non-skipped pending parts; no part left ⇒ red `NoParts`. `is_green` (commit all green), `can_commit` (not red), `effective_target`.
- HTTP bodies (`dto.rs`, section "Mass import"): `NewImportBatch`, `UpdateImportBatch` (absent = unchanged), `ImportBatch` (+ `ImportFileCounts`), `RegisterImportFiles`/`ImportFileInput` ⇒ `RegisteredImportFiles` (`upload: Vec<ImportUpload {id, path}>`), `ImportReview {batch, pieces: Vec<ReviewPiece>, unmatched, duplicates: Vec<StagedFile>}`, `UpdateImportItems {ids, piece?: PieceOverrides, part?: PartOverrides, reset}` (merge semantics: `PieceOverrides::merge`/`PartOverrides::merge`; `PartOverrides.skip: Option<bool>`), `CommitImport {piece_ids?}` ⇒ `CommittedImport {committed, errors: Vec<ImportItemError>}` (message = `ReviewPiece::blocking_note`), `UndoneImport {pieces, parts}`, `InstrumentAlias`/`InstrumentAliasInput` (alias normalised by `parts::alias_key`).
- JSON shapes: `Level`/`Source`/`FileState`/`ItemState`/`Language` lowercase strings; `Note` = `{"kind": snake_case, "detail": …}`; `Target` = `"new"` or `{"existing": id}`.

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
