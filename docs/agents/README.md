# docs/agents — technical documentation for coding agents

Optimised for agents, not for humans: dense, exhaustive, link-heavy. Describes the code **as it is now**.
Human-oriented docs: `docs/technical` (Typst, submodule). Requirements: `docs/requirements` (submodule).

## Files (read in this order for a first overview)

| File | Content | Read when |
| --- | --- | --- |
| `overview.md` | System, components, data flow, global invariants INV-* | always first |
| `background.md` | Brief, assumptions, architectures considered and rejected, PostgreSQL evaluation, all measurements, risks, open questions | before design decisions or proposing new infrastructure |
| `code-map.md` | Every source file → purpose, key symbols, requirements, decisions | before touching any file |
| `decisions.md` | Design decisions `d-*` with reasons, requirements served, change rules | before changing structure or behaviour |
| `server.md` | Server modules, request pipeline, full endpoint table | server work |
| `data-model.md` | SQLite schema, constraints, migration rules | schema or query work |
| `api-and-ink.md` | `musicus-api` crate: DTOs, status codes, binary ink format, merge | API or annotation work |
| `web-client.md` | Dioxus client: routes, session, bridge.js, reader internals | client work |
| `workflows.md` | Build/test commands, environment pitfalls, recipes, definition of done | before committing |
| `status.md` | Implemented vs. missing, test gaps, known debt | planning new work |
| `check.py` | Consistency checker for this folder | run before every commit |

## Reference syntax used everywhere

- Requirement IDs: `G-001` (goal, L1), `F-ACL-001` (feature, L2), `R-ACL-001` (requirement, L3).
  File: `docs/requirements/{goals|features}/<ID>.md` or `docs/requirements/requirements/<area>/<ID>.md`
  (`<area>` = middle part lowercased, e.g. `R-ACL-001` → `requirements/acl/R-ACL-001.md`).
  Machine-readable with statements and all links incl. reverse links: `docs/requirements/requirements.json`.
- Decision IDs: `d-<name>` (same labels as the decision records in `docs/technical/chapters/10-decisions.typ`,
  plus agent-level decisions `a-<name>` that exist only here). Defined in `decisions.md` as `### d-<name>`.
- Invariants: `INV-<n>` defined in `overview.md`.
- Code: repository-relative paths; symbols as `path::symbol`.

## Maintenance protocol (mandatory, see /AGENTS.md)

Same commit as the code change:

1. Update every statement here that the change makes false. Search for the changed symbol/path:
   `grep -rn "<symbol or path>" docs/agents`.
2. New/removed/renamed source file → update `code-map.md` (checked by `check.py`).
3. New endpoint/status code → `server.md` endpoint table and `api-and-ink.md`.
4. Schema change → `data-model.md` + new migration file.
5. New non-obvious choice → `decisions.md` entry (`a-<name>`; or `d-<name>` if also added to the Typst
   decision records) listing requirement IDs it serves and what breaks if changed.
6. Feature done / gap closed / new gap → `status.md`; requirement status in `docs/requirements`.
7. Run `python3 docs/agents/check.py` (and `--since <base>` before pushing).

`check.py` verifies: every source file listed in the code map; every listed path exists; every
requirement ID mentioned exists and is not deprecated; every `d-`/`a-` decision referenced is defined;
every `d-` label exists in the technical docs; with `--since REF`: code changed ⇒ `docs/agents/` changed.
