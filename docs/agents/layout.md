# System layout and system view (F-READ-003)

Detection of music systems on the server (`crates/musicus-server/src/layout/`), the shared reading
sequence / view planner (`crates/musicus-api/src/layout.rs`) and the system view of the reader
(`crates/musicus-web/src/pages/viewer.rs`, `layouts.rs`, `ink.rs`, `js/bridge.js`). Human version with
diagrams: `docs/technical/chapters/12-system-view.typ`. Prototype, measurements and the evaluation that led here:
`background.md` §6.1, `prototypes/system-detect`. Decisions: d-system-layout, a-layout-worker, a-css-mm,
a-footer-attach, a-view-anchor.

Constants are named in code; this file and the Typst chapter use the same names. Change both docs when you change
one. Raise `layout::ALGORITHM_VERSION` whenever detection results change (the worker re-analyses all files).

## Data flow

```
upload (library::store_upload) ──new files row──▶ app.layout_jobs.notify_one()
layout::worker (1 task, start-up + notify): next file without layout or with algorithm < ALGORITHM_VERSION,
  newest first ─▶ spawn_blocking(analyse_pdf) ─▶ INSERT/UPDATE file_layouts(file_id, algorithm, state, pages JSON)
GET /api/files/{sha256}/layout (ACL as the file) ─▶ FileLayout {state pending|done|failed, algorithm, pages}
client layouts::load (memory cache; programme view loads all ahead, R-READ-019)
  ─▶ lay::reading_items(pages) ─▶ lay::plan_views(items, Screen, StaffSize::min_staff_mm) ─▶ crops ─▶ bridge.renderCrop
```

`file_layouts` row missing ⇒ `pending` (algorithm 0, no pages). A row with an older `algorithm` is still served
(state `done`) until re-analysed. `failed` (hayro cannot read / panic) is stored and retried only after a version raise.

## Rendering (`layout/mod.rs::analyse_pdf`)

hayro 0.8 renders each page (rotation applied, `render_dimensions` in pt) to RGBA, white background, at
`RENDER_DPI` = 150, but scaled down so the long side is ≤ `MAX_LONG_SIDE_PX` = 150/72·842 ≈ 1754 px (facsimiles that
declare 532×810 mm pages). Grey = (77 R + 150 G + 29 B) >> 8. Page size in mm = pt · 25.4/72.
`analyse_page`: image < 16 px ⇒ whole page; else `detect::detect`, regions converted to `musicus_api::layout::Region`,
`full_page = confidence < CONFIDENCE_MIN (0.6) || no system`.

## Detection (`layout/detect.rs::detect`) — exact steps

Notation: `w×h` image, `t` line thickness, `d` line distance (centre to centre), "spaces" = multiples of `d`.

1. **Binarise**: Otsu threshold on the 256-bin histogram, clamped to `OTSU_MIN`=60..`OTSU_MAX`=210; ink = grey < thr.
2. **Run lengths** (every 2nd column): histogram of vertical black runs → `t` = most frequent length in 1..19;
   histogram of (black run + following white run) → `d` = argmax over 4..149 of `h[i-1]+2h[i]+h[i+1]`.
3. **Thin pixels**: ink in vertical runs ≤ `tmax` = max(`THIN_RUN_FACTOR`(2)·t+1, t+`THIN_RUN_EXTRA`(3)).
4. **Skew**: (subsample to ≤ 400 000 thin pixels; < 1000 ⇒ 0°) maximise Σ(row histogram²) of y − (x − w/2)·tan α over
   α ∈ [−`SKEW_MAX_DEG`, +3°] step `SKEW_COARSE_STEP` 0.1°, then ±0.1° around the best step `SKEW_FINE_STEP` 0.01°.
   |α| < `SKEW_IGNORE_DEG` 0.02° ⇒ none. Shear the ink image: y' = y − round((x − w/2)·tan α).
5. **Line evidence**: ink in horizontal runs ≥ max(`LONG_RUN_SPACES`·d = 1.5 d, `LONG_RUN_MIN_PX` 8) with gaps of
   `LONG_RUN_BRIDGE_PX` 1 closed (sheared image). Staff lines run through heads/stems; text does not.
6. **Strips**: width sw = clamp(`STRIP_SPACES`·d = 8 d, 32, max(w/4, 32)); per strip, rows with evidence ≥
   `MIN_LINE_FILL` 0.5·sw form runs; a run ≤ 3·tmax rows is a candidate (y = weighted centre, strength = peak/sw).
7. **Groups** per strip: for candidate pairs (i, l) with g = (y_l − y_i)/4 ∈ [max(`GROUP_MIN_SPACING` 0.4 d, 3 px),
   `GROUP_MAX_SPACING` 2.5 d], look for lines 1..3 at y_i + k·g within tol = max(`LINE_TOL` 0.2·g, 1.5 px); at most one
   interior line may be missing (counted as the median strength missing in the score). Reject if the weakest line <
   `MIN_LINE_STRENGTH` 0.45 × median, or if candidates lie within `MID_LINE_TOL` 0.2·g of the middle of ≥ 2 gaps
   (every-other-line across two staves). A sixth line at y_i + 5g with strength ≥ `TAB_LINE_STRENGTH` 0.7 × median
   gives an additional 6-line (tablature) group. Score = Σ strengths; choose greedily by score, non-overlapping
   (±g/2).
8. **Tracks**: left→right, a group joins the track with the nearest top line among tracks with the same line count,
   last strip ≥ k − `TRACK_MAX_GAP_STRIPS` 4, |Δtop| < `TRACK_Y_TOL` 0.6 g, |Δg| < `TRACK_SPACING_TOL` 0.2 g (one group per
   track and strip). Tracks with the same line count, |Δ median top| < 0.6 g and disjoint strips are merged. Staff =
   track spanning ≥ `MIN_STAFF_SPACES` 12 d and ≥ 2 strips; line positions = per-line medians; samples (strip centre,
   top) keep curvature. Overlapping staves: the one with more strips wins. A 6-line staff with spacing <
   `TAB_MIN_SPACING` 1.25 × median 5-line spacing loses its weaker outer line (staff + ledger line).
9. **Extent**: from the outer strip centres walk outwards per column while ≥ lines − 1 lines have ink within
   ±r (r = ceil(t/2)+1, curvature offset from the nearest sample); gaps ≤ `EXTENT_GAP_SPACES` 0.6 spaces + 1 px.
10. **Systems**: consecutive staves a, b are one system if the gap rows (a.bottom + r + 1 .. b.top − r) have ink in the
    window [left − `CONNECT_LEFT_SPACES` 3 d, left + `CONNECT_RIGHT_SPACES` 1 d] (left = min x0, d = larger spacing) in
    ≥ `MIN_CONNECT` 0.9 of the rows (brace/bracket/system barline), or some column x in the common width has ink within
    ±(t/2+1) in ≥ `BARLINE_COVER` 0.97 of the rows (barline, e.g. after an incipit).
11. **Row profile** over [min x0 − `CONTENT_MARGIN_SPACES` 2 d, max x1 + 2 d]; a row is ink-free if ≤ max(1,
    `NOISE_FRACTION` 0.004 × width) pixels. Scanner border: rows in the outer quarter with > `BORDER_ROW_FILL` 0.5 of
    the full width inked count as page edge.
12. **Cuts between systems**: in (bottom_prev + 2r, top_next − 2r) the centre of the longest ink-free run (tie: nearer
    the middle); none ⇒ the row with minimum 3-row ink sum (tie: nearer the middle), "clean" if its ink ≤
    `CUT_INK_OK` 0.02 × width.
13. **Cut above the first / below the last system**: ink-free runs between page edge and staff; score = length /
    (1 + distance to the staff / (`MARGIN_DECAY_SPACES` 8 d)); runs touching the page edge (or border) score −1 and
    yield a cut `EDGE_CUT_SPACES` 2 d from the ink; no run at all ⇒ step 12 within max(`MARGIN_GAP_FACTOR` 0.6 × typical
    gap, `MARGIN_MIN_SPACES` 5 d) (typical gap = median system gap, `DEFAULT_GAP_SPACES` 8 d if one system).
    **Footer attach** (a-footer-attach): if the first ink below the bottom cut starts < `FOOTER_ATTACH_SPACES` 4 d
    below the last staff, the cut moves below that ink block (block ends at an ink-free run of 1 d; cut in its middle).
14. **Regions** (deskewed, mapped back as bounding boxes: y ± (x − w/2)·tan α over the region's x range): header = first
    ink row .. first cut; system k = cut k .. cut k+1, x = ink columns (> max(1, `INK_COLUMN_FILL` 0.002 × rows))
    within [staves x0 − `SYSTEM_LEFT_SPACES` 12 d, x1 + `SYSTEM_RIGHT_SPACES` 6 d], staff_space = smallest spacing / h;
    footer = last cut .. last ink row. Bands are contiguous (y1 of one = y0 of the next).
15. **Confidence** (product): no staves ⇒ `NO_STAFF_CONFIDENCE` 0.2 if ink > `BLANK_INK` 0.3 % (else 1, "blank"); unclean
    cuts (between systems + above the first) × `PENALTY_UNCLEAN_CUT` 0.85 each, max 3; per system, outside
    `NEAR_STAFF_SPACES` 7 d of its staves: ≥ 3 separate rows with evidence ≥ `MISSED_LINE_FILL` 0.25 × width ⇒ ×
    `PENALTY_MISSED_STAFF` 0.5, else a run > 2 d of rows with ink > `FAR_INK_FILL` 0.05 × width ⇒ × `PENALTY_FAR_INK`
    0.9; systems whose staff count ≠ the median × `PENALTY_STAFF_COUNT` 0.8 each (max 3); a staff narrower than
    `MIN_WIDTH_RATIO` 0.85 × the widest of its system ⇒ × `PENALTY_WIDTHS` 0.7 per system; a 5-line staff spacing off
    the median by > `SPACING_DEVIATION` 30 % ⇒ × `PENALTY_SPACING` 0.9 (once); |skew| > `SKEW_WARN_DEG` 1.5° ⇒ ×
    `PENALTY_SKEW` 0.9.

Not detected by design: one-line percussion staves. Untested: two short systems side by side, ossia staves, rotated
pages. Known misses: footnotes below the last system are attached to it (harmless); manuscripts with darker paper.

## Reading sequence (`musicus_api::layout::reading_items`)

Per page in order: `full_page` or no system ⇒ one `Page` item (whole page). Else per region: System ⇒ item;
Header kept on the first page, Header/Footer kept if `ink_y1 − ink_y0 > TALL_TEXT` (0.06) (title block, verses) ⇒
`Text`; Hidden and small header/footer skipped. Crop y = ink ± (`SYSTEM_PADDING_SPACES` 1 × staff_space for systems,
0.005 for text), clamped to the cut lines; x = region x. `staff_mm` = 4 · staff_space · page height mm.

## Views (`plan_views`, `view_for_item`, `first_item_of_page`)

Screen = stage size − `STAGE_MARGIN` 8 px (CSS px), `px_per_mm` = `CSS_PX_PER_MM` = 132/25.4 (a-css-mm).
The items only decide where a view starts and ends; the view shows the ORIGINAL page (a-contiguous-view):

- One horizontal range for the whole document: [dx0, dx1] = union of x0·page_w .. x1·page_w (mm) of all non-`Page`
  items; `fit_w` = W / (dx1 − dx0). One scale for the document: `needed` = min_staff_mm · px_per_mm / smallest
  staff_mm of the document, `s` = min(needed, fit_w) (`fit_w` if no staff). Views never change scale or move sideways;
  too wide music is shown at full width (`below_min`).
- `View.segments`: per page touched by items first..=last, ONE `Segment {page, x0, x1, y0, y1}` (0..1): x = dx0/page_w ..
  dx1/page_w (clamped), y = top of the first item on that page .. bottom of the last (everything in between is shown:
  gaps, text, hidden bands, skipped small headers/footers between systems). A `Page` item = segment 0..1 × 0..1.
- Height of s..=e = Σ over segments (y1 − y0) · page_h mm. Grow e while the next item is not a `Page` item and
  height × s ≤ H. Then drop trailing header-like `Text` (next item on the same page or text in the upper half).
  `Page` items are alone, scale = min(W/page_w, H/page_h). A single item taller than H at `s` gets scale H/height.
  `below_min` = scale < needed. A view consisting only of the overlapped system is skipped.
- Next start = last item if the view has ≥ `OVERLAP_MIN_SYSTEMS` 3 systems and ends with a system (one-system overlap),
  else last + 1 (user decision).
Reading position (a-view-anchor): the reader stores the first item of the current view (`anchor`) and shows
`view_for_item` = last view starting at or before it; re-planning (rotation, size change) keeps the anchor. Page
view → system view: anchor = `first_item_of_page(current page)`; the current page follows the first segment.

## Client (`viewer.rs`)

Signals/memos: `settings` (from `Session.user.reader`; changes PUT `/api/auth/reader-settings` and update the
session), `layout` (loaded per displayed file URL, retried every `LAYOUT_RETRY_MS` 10 s while pending, ≤ 30 times),
`items`, `views` (empty unless mode Systems and layout done with page count = pdf.js page count), `view_idx`, `crops`
(`view_crops`: one per `View.segments` entry — CSS width = (x1 − x0) × page_w_mm × scale, `PageBox::crop`).
DOM: `.system-view` (one outer shadow, `data-view="i/n"`, `data-items`) holding one `.page-segment` per page (PDF
canvas `crop-{k}` + ink canvas `crop-ink-{k}`), directly stacked; no shadow, border or gap between systems; a 1 px
`rgba(0,0,0,.12)` hairline (`::before`, no layout effect) only where a view continues on the next page. A stroke drawn
across several systems of a page is one stroke on one canvas, straight in page coordinates; it cannot continue onto
the next page (another canvas, another page).
Effects: page render only when not in system view; crop render (`bridge::render_crop` per segment into `crop-{k}`,
then `prerender_crop` for the next view); annotation load for every page shown; ink redraw per segment into
`crop-ink-{k}` with its `PageBox` (offset + visible size, `ink::prepare` translates by −offset). Pointer input
carries a `Target {page, bx, canvas}`; coordinates are converted with `PageBox::canvas_to_page`, so stored ink
stays page-relative (R-ANN-005, R-ANN-012). Undo/redo in the system view act on the page last drawn on (`edit_page`).
`turn(±1)`: views (anchor = next/previous view's first item), before the first / after the last view the
previous / next piece. Swipe: pointer down/up on the stage in read mode, |dx| ≥ `SWIPE_MIN_PX` 60 and |dx| > 2|dy|;
the click that ends a swipe is ignored (400 ms). `.system-view[data-view="i/n"]` is for tests.
Notes: pending/failed layout in system mode ⇒ blue banner and page view.

`bridge.js`: `renderCrop(url, page, canvasId, x0, y0, x1, y1, cssWidth)` renders only the crop with a pdf.js viewport
offset (scale = cssWidth / (crop width · page width pt)), canvas at devicePixelRatio, cache `crops` (LRU 24 bitmaps,
key = url|page|crop|round(cssWidth)|dpr; also used by `prerenderCrop`), newest request per canvas wins;
`retainOnly` drops a file's crops.

## Corrections (R-READ-016, a-layout-corrections)

`PageCorrection {full_page, bounds, kinds}`: band i = `bounds[i]..bounds[i+1]` (0..1 of the page height) of kind
`kinds[i]`; nothing outside the first/last bound is shown. `validate`: ≤ `MAX_CORRECTED_BANDS` 60 bands; full_page
may have no bands; else `bounds.len() == kinds.len() + 1`, values in 0..1, each band ≥ `MIN_BAND_HEIGHT` 0.005.
Stored per (file, page) in `layout_corrections`, applied when serving (`files::effective_layout` →
`layout::apply_correction`): per band, x range / ink rows = union of the detected regions it overlaps (else whole
width / the band); an edge that is not a detected cut line (|Δ| ≥ `CUT_EPSILON` 1e-4, i.e. the librarian moved or
added it) sets the ink row itself, so the band is shown up to that line even across empty paper; staves = sum of detected systems overlapping it by more than half (≥ 1 for a System band),
staff_space = smallest of the overlapped systems (else the page's smallest); `confidence` 1, `corrected` true,
`full_page` = correction's flag or no System band with a staff size. The worker never touches corrections, so they
survive re-analysis. Editor: `pages/layout_edit.rs` (see `web-client.md`): draft from the effective page (regions →
bounds/kinds; no regions ⇒ one System band 0..1), Split inserts a cut in the middle, × removes a cut (the joined
band is System if either was), cut drag clamps between neighbours ± `MIN_BAND_HEIGHT`; Save/Reset call `layouts::forget`
so this tab's readers refetch (other devices load the layout when they open the part again).

## Regression numbers (local example set, not committed)

`cargo run --release -p musicus-server --example layout_report -- <pdfs>`: on the 23 example PDFs (all 226 pages) the
production code gives the same systems and staves per page as the prototype on every page judged there (76 pages +
16 manuscript pages), except one blank page that is now marked whole. Time per page incl. rendering, mean per file:
23–79 ms for printed music and scans, 112 ms for the 100 MB manuscript (Ryzen 7 5800H, one core).
