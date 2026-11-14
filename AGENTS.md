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
3. Before changing a design decision (`d-…` in `docs/agents/decisions.md`), check which
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
  `uv run docs/requirements/tools/validate.py` (commit inside the submodule, then the pointer);
- tell the user if `docs/technical` (human docs, Typst) is now outdated, or update it.

Then run the checks below. `python3 docs/agents/check.py --since <base-ref>` fails if code changed
without any change under `docs/agents/`. If a change truly needs no doc update (pure formatting,
comment typo), set `AGENT_DOCS_UNCHANGED="<reason>"` for that run and say so in the commit message.

## Commands

```sh
cargo test                                                   # api + server unit + API integration tests
cargo clippy --all-targets && cargo fmt --check
cargo clippy -p musicus-web --target wasm32-unknown-unknown  # client only builds for wasm32
./scripts/fetch-pdfjs.sh && dx build --platform web --package musicus-web
python3 docs/agents/check.py                                 # agent-doc consistency
uv run docs/requirements/tools/validate.py                   # requirements consistency
```

Optional git hook that runs the doc check before every commit: `git config core.hooksPath .githooks`.

## Hard rules (details and reasons in docs/agents/)

- Every query touching parts/scores/files/programmes/annotations must use `part_visible!()`
  (R-ACL-001, R-ACL-002). Denied → 404, never 403.
- SQL strings are `&'static str`; pass id lists via `json_each(?)` (R-SEC-006).
- Blobs are immutable; never modify or delete files in `blobs/` (R-FILE-004).
- Annotation objects are immutable (edit = remove + add) — the merge depends on it (R-ANN-012).
- Writes go through `app.db.write` (single connection), reads through `app.db.read` (R-PERF-006).
- The web client must not persist data on the device (R-SEC-001).
- New migrations: new numbered file in `crates/musicus-server/migrations/`; never edit applied ones.
