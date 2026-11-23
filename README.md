# Musicus

Musicus provides sheet music to the members of an orchestra or band. A
librarian uploads PDF parts. Access is granted to groups (e.g. group
"Trumpets" reads every part tagged with the instrument "Trumpet"). Every
member can draw private annotations on their parts, in several named sets
(e.g. one per concert), without changing the PDF.

Architecture: one Rust binary (Axum + SQLite in WAL mode + a
content-addressed file store) that also serves a Dioxus web client.

```
crates/
  musicus-api/     shared types: JSON bodies and the annotation (ink) format
  musicus-server/  Axum server, SQLite database updates, file store, CLI
  musicus-web/     Dioxus 0.7 web client (WebAssembly) with a pdf.js bridge
e2e/               browser smoke test (Playwright)
prototypes/        throw-away benchmarks behind the design
docs/technical/    technical documentation (Typst, git submodule)
docs/requirements/ requirements catalogue (Markdown, git submodule)
docs/agents/       technical documentation for coding agents (see AGENTS.md)
docs/user-guide/   user guide: installation and use (Typst → HTML + PDF, git submodule)
scripts/           fetch-pdfjs.sh
```

## Documentation

The user guide (installation, and how admins, librarians and musicians use
Musicus) is the submodule `docs/user-guide`; build it with
`docs/user-guide/build.sh` (HTML and PDF).

The technical documentation (architecture, components, design decisions) is
the submodule `docs/technical`, written in Typst; build it with
`docs/technical/build.sh` (PDF, the main format, and HTML):

```sh
git submodule update --init
docs/technical/build.sh      # → docs/technical/out/musicus-technical.pdf and .html
```

The requirements (goals, features, detailed requirements and their links)
are the submodule `docs/requirements`; start with its `index.md`. After
editing them, run `uv run docs/requirements/tools/validate.py`.

The three documentation submodules track their `main` branch. A change to
them is merged in its own repository first; afterwards a comment
`/update-submodules` on the main pull request lets a bot move the pointers to
the merge commits and push them (or run `./scripts/update-submodules.sh`,
commit and push yourself). The pull-request check `scripts/check-submodules.sh`
ensures the main repository only references commits on the docs
repositories' `main`. The bot needs a one-time setup of a GitHub App:
[docs/github-setup.md](docs/github-setup.md).

Coding agents start at `AGENTS.md`, which points to `docs/agents/`. That
documentation must be updated in the same commit as every code change;
`python3 docs/agents/check.py --since <base>` enforces it (also available as a
pre-commit hook: `git config core.hooksPath .githooks`, and as a GitHub
workflow that runs on pull requests and must pass before merging).

## Build and run

Prerequisites: Rust 1.88 or newer, the `wasm32-unknown-unknown` target and
the Dioxus CLI 0.7.10 (`dx`).

```sh
rustup target add wasm32-unknown-unknown
cargo install dioxus-cli --version 0.7.10 --locked   # or a prebuilt `dx` release

# 1. pdf.js (pinned, checksum-verified) for the web client
./scripts/fetch-pdfjs.sh

# 2. the web client -> target/dx/musicus-web/release/web/public
rm -rf target/dx/musicus-web/release        # no files of an earlier build
dx bundle --platform web --package musicus-web --release

# 3. the server, with the web client built in: one file
cargo build --release -p musicus-server --features embed-web
./target/release/musicus-server serve --data-dir ./data --public-url http://localhost:8080
```

`target/release/musicus-server` is then the whole program; copy it anywhere.
Without `--data-dir` it keeps its data in the usual folder of the system
(Linux `~/.local/share/musicus`, macOS `~/Library/Application Support/Musicus`,
Windows `%APPDATA%\Musicus`) and prints the folder at start. Without
`--features embed-web` the program contains no web client; pass
`--web-dir <folder of a built client>` then (also handy in development).

On the first start the server creates the user `admin` and logs a one-time
link to choose its password. Open it, then use **Manage** to add instruments,
scores, parts (upload the PDFs), groups and users. New users get an invite
link instead of a password; send it to them, and they choose their own.

For development, `dx build --platform web --package musicus-web` builds a
debug client into `target/dx/musicus-web/debug/web/public`. Release builds
run `wasm-opt`, which `dx` downloads on first use.

## Docker

The `Dockerfile` builds the program with the web client built in and puts it
into a small image based on distroless (no shell, runs as an unprivileged user,
data in the volume `/data`). `compose.yaml` runs it:

```sh
cp .env.example .env                  # optional: adjust settings
docker compose up -d --build
docker compose logs musicus           # the one-time admin setup link
```

The web client is then on http://localhost:8080; the port is published on
every address over IPv4 and IPv6 (`MUSICUS_LOCAL_ONLY` empty). `MUSICUS_LOCAL_ONLY=true`
publishes it on `127.0.0.1` and `[::1]` only, for a proxy on the same machine
(either address, or `localhost`, works). Other addresses: a
`compose.override.yaml` with `ports: !override` (user guide). Tablets and phones on the
network need HTTPS (see below). With a domain that points to the machine,
set `MUSICUS_DOMAIN`, `MUSICUS_PUBLIC_URL`, `MUSICUS_LOCAL_ONLY=true` and
`MUSICUS_TRUST_PROXY=true` in `.env`, then start with Caddy in front. Caddy
fetches and renews the Let's Encrypt certificate itself:

```sh
docker compose --profile https up -d --build
```

Backups while running: `docker compose exec musicus musicus-server backup --to
/data/backup-$(date +%F).sqlite`, then copy the volume's `blobs/` directory.
To restore: `docker compose stop musicus`, then
`docker compose run --rm musicus restore --from /data/backup-….sqlite`.
The image has a health check (`musicus-server healthcheck`, which calls
`GET /api/health`). For a Raspberry Pi, build on the Pi or use
`docker buildx build --platform linux/arm64`. The first build takes a while,
because it compiles the Dioxus CLI.

The build downloads `dx`'s helper tools (wasm-bindgen, wasm-opt, esbuild).
Where that is not possible (offline or behind a TLS-intercepting proxy),
use a base image with those three on `PATH` and `NO_DOWNLOADS=1` set, and
pass it as `--build-arg RUST_IMAGE=<image>`.


All options can be passed as flags or environment variables
(`musicus-server serve --help`).

| Variable | Default | Meaning |
| --- | --- | --- |
| `MUSICUS_DATA_DIR` | usual folder of the system (see above); `/data` in Docker | Holds `musicus.sqlite`, `blobs/` and `tmp/` |
| `MUSICUS_LISTEN` | `[::]:8080` | Address and port; `[::]` accepts IPv6 and IPv4 (falls back to `0.0.0.0` without IPv6), any other address is used as given |
| `MUSICUS_WEB_DIR` | none | Folder of a built web client, used instead of the one built into the program |
| `MUSICUS_PUBLIC_URL` | `http://localhost:8080` | Used in printed setup links |
| `MUSICUS_TLS_CERT`, `MUSICUS_TLS_KEY` | none | PEM files: serve HTTPS directly (rustls) |
| `MUSICUS_INSECURE_COOKIES` | off | Drop the cookie's `Secure` flag (plain-HTTP development on a LAN IP only) |
| `MUSICUS_CLIENT_CACHE` | `none` | `none`: browsers may not store sheets (`no-store`); `immutable`: cache privately for a year |
| `MUSICUS_RAM_CACHE_MB` | 128 | RAM for hot sheet files (tonight's programme) |
| `MUSICUS_MAX_UPLOAD_MB` | 200 | Largest upload |
| `MUSICUS_SESSION_DAYS` | 60 | Sliding session lifetime, so tablets stay logged in |
| `MUSICUS_TRUST_PROXY` | off | Read the client IP from `X-Forwarded-For` (behind a reverse proxy) |

Log levels follow `RUST_LOG` (default `info`).

### Deployment notes

- **HTTPS is required in practice.** Browsers only allow the screen wake lock
  and the `Secure` session cookie in secure contexts. Either pass a
  certificate or put Caddy/nginx in front. On a Raspberry Pi without internet
  access, use a real domain with a DNS-01 certificate.
- **Backups:** `musicus-server backup --data-dir data --to backup.sqlite`
  writes a consistent copy while the server runs. Then back up `data/blobs/`.
  Files there are immutable, so incremental tools (restic, rsync) work well.
  `musicus-server restore --data-dir data --from backup.sqlite` (server
  stopped) puts a backup back. Before database updates Musicus keeps a
  snapshot in `data/snapshots/` and restores it by itself if an update fails.
  Backups and snapshots are unencrypted: store them as safely as the server.
- **Resources:** the release binary idles at about 11 MB RSS. The prototypes
  served 120 simultaneous downloads of a 3 MB part on one core in 0.2 s; the
  rehearsal-room Wi-Fi is the real limit, which is why the client downloads
  a whole programme ahead of time.

## How it works

- **Access control:** a member may read a part if one of their groups is
  granted one of the part's instruments, or the whole score (e.g. for the
  conductor). Librarians and admins read everything. Denied reads answer 404.
  The check is one indexed SQL query per request; revocations apply at once.
- **Files** are stored as `blobs/ab/<sha256>` and served at
  `/api/files/<sha256>` with `ETag`, `Range` support and a RAM cache.
  Uploading a corrected PDF creates a new version of the part. Old versions
  stay readable for annotations drawn on them, and a set can be moved to the
  new version when the page count is unchanged.
- **Annotations** are vector objects (pen and highlighter strokes with
  pressure, stamps, text) in page-relative coordinates. They are encoded with
  postcard plus delta coding (about 7 KB for a typical page) and stored
  zstd-compressed per page. Saves use `If-Match` revisions. A conflicting
  save from another device answers 409, and the client merges by object id.
  Pages are saved 1.5 s after the last stroke, on page turns, and (with a
  `keepalive` request) when the tab is closed.
- **The client** keeps nothing on the device. Opening a programme downloads
  all its sheets into memory, so page turns and the next piece need no
  network. The reader turns pages by tap, arrow/Page keys or a Bluetooth foot
  pedal, keeps the screen awake, and ignores fingers while a pen is used.
- **Accounts:** Argon2id (19 MiB, t = 2) with a semaphore bounding parallel
  hashing. Opaque session tokens are stored hashed: an `HttpOnly` cookie plus
  a CSRF header for the web, bearer tokens for future apps. Failed logins are
  limited (10 per 15 minutes per account and per IP).

## API overview

All endpoints are under `/api`; the JSON bodies are defined in
`crates/musicus-api/src/dto.rs`.

| Area | Endpoints |
| --- | --- |
| Session | `POST auth/login`, `POST auth/logout`, `GET auth/me`, `POST auth/password`, `GET auth/sessions`, `DELETE auth/sessions/{id}`, `GET/POST auth/invites/{token}` |
| Users (admin) | `GET/POST users`, `PATCH users/{id}`, `POST users/{id}/invite` |
| Instruments | `GET/POST instruments`, `PUT/DELETE instruments/{id}` |
| Groups | `GET/POST groups`, `PUT/DELETE groups/{id}`, `PUT groups/{id}/members`, `PUT groups/{id}/instrument-grants`, `PUT groups/{id}/score-grants` |
| Library | `GET scores?q=&genre=&tag=`, `POST scores`, `GET/PUT/DELETE scores/{id}`, `POST scores/{id}/parts`, `GET/PUT/DELETE parts/{id}`, `PUT parts/{id}/file` (raw PDF body), `GET parts/{id}/versions` |
| Files | `GET files/{sha256}`, `GET files/{sha256}/thumbnail` |
| Health | `GET health` (no login; checks the database) |
| Programmes | `GET/POST programmes`, `GET/PUT/DELETE programmes/{id}`, `PUT programmes/{id}/items` |
| Annotations | `GET/POST parts/{id}/annotation-sets`, `PATCH/DELETE annotation-sets/{id}`, `POST annotation-sets/{id}/migrate`, `GET/PUT annotation-sets/{id}/pages/{page}` (binary, `If-Match`) |

## Tests

```sh
cargo test                     # shared types, server unit and API tests
cargo clippy --all-targets
cargo clippy -p musicus-web --target wasm32-unknown-unknown
```

`e2e/smoke.mjs` drives the whole application in Chromium. It covers admin
setup, upload, groups, invites, search, programme prefetch, reading, drawing,
saving, reload and page turns:

```sh
(cd e2e && npm install && npx playwright install chromium)
./target/debug/musicus-server serve --data-dir /tmp/musicus-e2e \
    --web-dir target/dx/musicus-web/debug/web/public --listen 127.0.0.1:8099
node e2e/smoke.mjs <invite-url-from-the-log> some.pdf /tmp/shots
```

## Status of the first version

Implemented: everything above, including the core metadata fields, concert
programmes, file versions, thumbnails and page counts via hayro, the backup
command, optional built-in TLS and the container setup.

Planned features are recorded as requirements with status `proposed` in
`docs/requirements` (see its `index.md`), e.g. finer grants, extended
metadata, MusicXML, the encrypted offline app, OpenID Connect login, shared
annotation sets and a conductor push. Known gaps and deviations from the
original design are listed in `docs/agents/status.md`; the design rationale
and all measurements in `docs/agents/background.md`.

## Releasing

How to make a release (kinds, release branch, merging the database updates, tags, release notes):
[RELEASING.md](RELEASING.md).

## License

    Musicus: sheet music for the members of an orchestra or band.
    Copyright (C) 2026  the Musicus contributors

    This program is free software: you can redistribute it and/or modify
    it under the terms of the GNU Affero General Public License as published
    by the Free Software Foundation, either version 3 of the License, or
    (at your option) any later version.

    This program is distributed in the hope that it will be useful,
    but WITHOUT ANY WARRANTY; without even the implied warranty of
    MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
    GNU Affero General Public License for more details.

    You should have received a copy of the GNU Affero General Public License
    along with this program.  If not, see <https://www.gnu.org/licenses/>.

The full license text is in `LICENSE`. The documentation in the submodules
`docs/requirements`, `docs/technical` and `docs/user-guide` is licensed under
the Creative Commons Attribution-ShareAlike 4.0 International License
(CC BY-SA 4.0); see the `LICENSE` file in each of them.
