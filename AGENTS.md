# AGENTS.md — mandatory instructions for coding agents

Musicus: sheet-music server (Rust/Axum/SQLite) + web client (Rust/Dioxus → WASM, pdf.js bridge).
Members read group-granted PDF parts and draw private annotations.

## Before you change anything

1. Read `docs/agents/README.md` (index, ~5 min). Then read the doc(s) for the area you touch
   (`docs/agents/code-map.md` tells you which).
2. Look up every requirement ID you encounter (`R-…`, `F-…`, `G-…`):
   `docs/requirements/requirements/<area lowercase>/<ID>.md` (level 3),
   `docs/requirements/features/<ID>.md`, `docs/requirements/goals/<ID>.md`,
   or search `docs/requirements/requirements.json`.
   Submodules missing? `git submodule update --init`.
3. Before proposing new infrastructure or changing architecture, read `docs/agents/background.md`
   (alternatives already evaluated and why they were rejected).
4. Before changing a design decision (`d-…` in `docs/agents/decisions.md`), check which
   requirements it serves. Do not break a requirement silently: change the requirement
   (with the user's consent) or keep the design.

## When you change the program (NOT optional)

Every commit that changes behaviour, structure, API, schema, configuration, build or tests MUST,
in the same commit:

- update the affected files in `docs/agents/` so they describe the code **as it is after the commit**
  (code map, module docs, endpoint table, data model, decisions, status);
- add new source files to `docs/agents/code-map.md` (the checker fails otherwise);
- add a decision to `docs/agents/decisions.md` when you choose something a reader would not expect,
  with the requirement IDs it serves;
- update requirement files in `docs/requirements` if behaviour changed (status, `implemented_by`,
  `verified_by`, new requirements for new behaviour), then run
  `uv run docs/requirements/tools/validate.py` (commit inside the submodule on a branch, then the pointer);
- tell the user if `docs/technical` (human docs, Typst) is now outdated, or update it;
- if the change is visible to users (UI labels, workflows, installation, settings), update
  `docs/user-guide` (Typst submodule; button names via `#ui("…")`) and regenerate its screenshots
  with `docs/user-guide/tools/screenshots.mjs` (see its README), or tell the user it is outdated.

Then run the checks below. `python3 docs/agents/check.py --since <base-ref>` fails if code changed
without any change under `docs/agents/`. If a change truly needs no doc update (pure formatting,
comment typo), set `AGENT_DOCS_UNCHANGED="<reason>"` for that run and say so in the commit message.

## Commands

```sh
cargo test                                                   # api + server unit + API + update-runner tests (needs python3)
cargo clippy --all-targets && cargo fmt --check
cargo clippy -p musicus-web --target wasm32-unknown-unknown  # client only builds for wasm32
./scripts/fetch-pdfjs.sh && dx build --platform web --package musicus-web
python3 scripts/check-ui-literals.py                          # no literal UI text in the client
scripts/run-e2e.sh smoke                                     # browser tests like CI (also german, pseudo, outage; see the script)
python3 docs/agents/check.py                                 # agent-doc consistency
uv run docs/requirements/tools/validate.py                   # requirements consistency
```

Optional git hook that runs the doc check before every commit: `git config core.hooksPath .githooks`.

## Submodules (docs/requirements, docs/technical, docs/user-guide)

Docs changes go to a branch in the submodule repository and get their own pull request there.
The main repository's pull request is merged LAST: after the docs pull requests are merged into
their `main`, run `./scripts/update-submodules.sh`, commit, and push (or comment `/update-submodules` on the
pull request: workflow `Update submodules` does the same), so that the pointers name the
merge commits on the docs `main` (never the feature-branch commits). The required check
`scripts/check-submodules.sh` (workflow `Submodules`) fails otherwise. Details: `docs/agents/workflows.md`.

## Hard rules (details and reasons in docs/agents/)

- Every query touching parts/scores/files/programmes/annotations must use `part_visible!()`
  (R-ACL-001, R-ACL-002). Denied → 404, never 403.
- SQL strings are `&'static str`; pass id lists via `json_each(?)` (R-SEC-006).
- Blobs are immutable; never modify or delete files in `blobs/` (R-FILE-004).
- Annotation objects are immutable (edit = remove + add) — the merge depends on it (R-ANN-012).
- Writes go through `app.db.write` (single connection), reads through `app.db.read` (R-PERF-006).
- The web client must not persist data on the device (R-SEC-001).
- Schema changes: a new preview `crates/musicus-server/migrations/preview/NNNN-<topic>.sql` (additive/rebuild/removal sections); never edit a merged preview or a released update;
  commit the regenerated `crates/musicus-server/schema.sql` (`./scripts/db-schema.sh`) with it (CI job `db-check`).
- Store and show user data exactly as entered; never translate or reformat it (a-data-as-entered, R-I18N-006).
  UI terms and their German translations: `docs/agents/glossary.md`.
- Every text the web client shows comes from the catalogue `crates/musicus-i18n/locales/en/*.ftl` via
  `musicus_i18n::t::…` — never a literal in `rsx!`, `confirm`/`prompt` or a status message (R-I18N-010; lint
  `scripts/check-ui-literals.py`). Ids `<area>-<page>-<what>`, never reused; changed meaning = new id (a-i18n-keys).
  Write only English; do not edit other languages except deleting obsolete ids.
- The server sends no composed names: parts carry their components; the web client composes names, key names and
  the unnamed set's name in the UI language (`names.rs`, `naming::NameWords`; a-client-part-names, R-I18N-006).
