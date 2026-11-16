#!/usr/bin/env bash
# Builds the user-guide website (published to GitHub Pages by
# .github/workflows/user-guide-pages.yml; runs locally too).
#
#   scripts/build-guide-site.sh [--out DIR] [--releases FILE] [--preview REF]
#
#   --out DIR        output directory, replaced completely (default: _site)
#   --releases FILE  release tags of Musicus, one per line ("-" = stdin).
#                    Default: every local git tag. CI passes the published,
#                    non-draft, non-prerelease releases.
#   --preview REF    additionally build REF (branch, tag or commit of Musicus)
#                    as a test build at /test-<REF>/, labelled as such.
#
# Every version is built from the user-guide commit that the Musicus commit's
# submodule pointer `docs/user-guide` names, with that commit's own build.sh.
# Releases whose commit has no `docs/user-guide` or whose build fails are
# skipped with a warning; the site is still built. Exits non-zero (nothing to
# deploy) if the --preview build fails or if no release with a user guide
# could be built at all (a-guide-site).
#
# Layout of DIR:
#   index.html                 list of all versions (test builds first, then releases newest first)
#   versions.json              the same as data
#   <tag>/index.html           guide of a release (+ musicus-user-guide.pdf if the build makes one)
#   latest/                    copy of the newest release (highest version number)
#   test-<ref>/                test build (only with --preview)
#
# Needs: git, typst (the version the guide's README names), python3, tar.
# The user-guide commits are taken from the local submodule checkout when it
# has them, otherwise fetched from the submodule URL (HTTPS in CI).
set -euo pipefail

OUT=_site
RELEASES_FILE=""
PREVIEW_REF=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --out) OUT="$2"; shift 2 ;;
        --releases) RELEASES_FILE="$2"; shift 2 ;;
        --preview) PREVIEW_REF="$2"; shift 2 ;;
        -h | --help) sed -n '2,/^set -euo/p' "$0" | sed '$d; s/^# \{0,1\}//'; exit 0 ;;
        *) echo "unknown argument: $1 (see --help)" >&2; exit 2 ;;
    esac
done

ROOT="$(git rev-parse --show-toplevel)"
SUB=docs/user-guide
[[ "$OUT" = /* ]] || OUT="$PWD/$OUT"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
GUIDE_GIT="$WORK/guide.git"
git init -q --bare "$GUIDE_GIT"
ENTRIES="$WORK/entries.tsv"
: >"$ENTRIES"

# In CI there is no SSH key; the docs repositories are public, use HTTPS.
GIT_URL_OPTS=()
if [[ -n "${GITHUB_ACTIONS:-}" ]]; then
    GIT_URL_OPTS=(-c "url.https://github.com/.insteadOf=git@github.com:")
fi

warn() {
    if [[ -n "${GITHUB_ACTIONS:-}" ]]; then echo "::warning::$*"; else echo "warning: $*" >&2; fi
}

slug() { printf '%s' "$1" | tr -c 'A-Za-z0-9._-' '-'; }

# Prints a repository (git dir) that contains user-guide commit $1, fetching it if needed.
guide_source() {
    local sha="$1" url="$2"
    if [[ -e "$ROOT/$SUB/.git" ]] && git -C "$ROOT/$SUB" cat-file -e "$sha^{commit}" 2>/dev/null; then
        git -C "$ROOT/$SUB" rev-parse --absolute-git-dir
        return
    fi
    if ! git -C "$GUIDE_GIT" cat-file -e "$sha^{commit}" 2>/dev/null; then
        git -C "$GUIDE_GIT" "${GIT_URL_OPTS[@]}" fetch -q --depth 1 "$url" "$sha" >&2 ||
            git -C "$GUIDE_GIT" "${GIT_URL_OPTS[@]}" fetch -q "$url" '+refs/heads/*:refs/heads/*' >&2 ||
            return 1
        git -C "$GUIDE_GIT" cat-file -e "$sha^{commit}" 2>/dev/null || return 1
    fi
    echo "$GUIDE_GIT"
}

# build_version <musicus-commit> <dir name> <kind: release|test> <label> <date>
# Returns 0 if built, 2 if the commit has no user guide, 1 if the build failed.
build_version() {
    local commit="$1" name="$2" kind="$3" label="$4" date="$5"
    local sha url src dir
    sha="$(git -C "$ROOT" ls-tree "$commit" "$SUB" | awk '$2 == "commit" {print $3}')"
    if [[ -z "$sha" ]]; then
        warn "$label: no $SUB at ${commit:0:12} (older than the user guide), skipped"
        return 2
    fi
    url="$(git -C "$ROOT" config --blob "$commit:.gitmodules" "submodule.$SUB.url" 2>/dev/null ||
        git -C "$ROOT" config -f "$ROOT/.gitmodules" "submodule.$SUB.url")"
    if ! src="$(guide_source "$sha" "$url")"; then
        warn "$label: user-guide commit ${sha:0:12} not found in $url, skipped"
        return 1
    fi
    dir="$WORK/src-$name"
    mkdir -p "$dir"
    git -C "$src" archive --format=tar "$sha" | tar -x -C "$dir"
    if ! (cd "$dir" && sh ./build.sh) >"$dir.log" 2>&1; then
        sed 's/^/    /' "$dir.log" >&2
        warn "$label: build of user-guide ${sha:0:12} failed, skipped"
        return 1
    fi
    local html pdf=""
    html="$(find "$dir/out" -maxdepth 1 -name '*.html' | head -n 1)"
    if [[ -z "$html" ]]; then
        warn "$label: build of user-guide ${sha:0:12} produced no HTML, skipped"
        return 1
    fi
    mkdir -p "$OUT/$name"
    cp "$html" "$OUT/$name/guide.raw.html"
    if [[ -f "$dir/out/musicus-user-guide.pdf" ]]; then
        cp "$dir/out/musicus-user-guide.pdf" "$OUT/$name/"
        pdf=musicus-user-guide.pdf
    fi
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$name" "$kind" "$label" "$date" "$commit" "$sha" "$pdf" >>"$ENTRIES"
    echo "built $name ($label, user guide ${sha:0:12})"
}

case "$OUT" in
    / | "$ROOT" | "$ROOT/" | "$HOME" | "$HOME/") echo "error: refusing to replace --out $OUT" >&2; exit 2 ;;
esac
[[ ! -e "$OUT/.git" ]] || { echo "error: refusing to replace --out $OUT (a git checkout)" >&2; exit 2; }
rm -rf "$OUT"
mkdir -p "$OUT"

# Test build first: it is listed on top.
if [[ -n "$PREVIEW_REF" ]]; then
    commit="$(git -C "$ROOT" rev-parse -q --verify "$PREVIEW_REF^{commit}" ||
        git -C "$ROOT" rev-parse -q --verify "origin/$PREVIEW_REF^{commit}")" ||
        { echo "error: --preview $PREVIEW_REF is not a commit, branch or tag" >&2; exit 1; }
    # Asked for explicitly: a test build that cannot be built fails the run.
    build_version "$commit" "test-$(slug "$PREVIEW_REF")" test "$PREVIEW_REF" \
        "$(git -C "$ROOT" log -1 --format=%cs "$commit")" ||
        { echo "error: test build of $PREVIEW_REF failed (see above)" >&2; exit 1; }
fi

# Releases, newest (highest version) first.
if [[ -z "$RELEASES_FILE" ]]; then
    tags="$(git -C "$ROOT" tag --list)"
elif [[ "$RELEASES_FILE" == - ]]; then
    tags="$(cat)"
else
    tags="$(cat "$RELEASES_FILE")"
fi
built=0 failed=0
while read -r tag; do
    [[ -n "$tag" ]] || continue
    if ! commit="$(git -C "$ROOT" rev-parse -q --verify "refs/tags/$tag^{commit}")"; then
        warn "release $tag: tag not found locally (fetch tags first), skipped"
        failed=$((failed + 1))
        continue
    fi
    rc=0
    build_version "$commit" "$(slug "$tag")" release "$tag" \
        "$(git -C "$ROOT" for-each-ref --format='%(creatordate:short)' "refs/tags/$tag")" || rc=$?
    case "$rc" in 0) built=$((built + 1)) ;; 1) failed=$((failed + 1)) ;; esac
done < <(printf '%s\n' "$tags" | sed '/^$/d' | sort -rV -u)
# Every release that has a user guide failed (e.g. Typst missing or incompatible):
# do not replace the published site by an empty one.
if [[ "$built" -eq 0 && "$failed" -gt 0 ]]; then
    echo "error: none of the $failed release(s) with a user guide could be built" >&2
    exit 1
fi

# Banners, latest/, index.html, versions.json.
python3 - "$OUT" "$ENTRIES" <<'PY'
import html, json, shutil, sys
from pathlib import Path

out, entries = Path(sys.argv[1]), Path(sys.argv[2])
cols = ["dir", "kind", "label", "date", "commit", "guide_commit", "pdf"]
versions = [dict(zip(cols, line.split("\t"))) for line in entries.read_text().splitlines() if line]
releases = [v for v in versions if v["kind"] == "release"]

BAR_CSS = """<style>
.musicus-site-bar { font: 14px/1.4 system-ui, -apple-system, "Segoe UI", Roboto, sans-serif;
  display: flex; flex-wrap: wrap; gap: 0.3rem 1rem; align-items: center; padding: 0.45rem 1.2rem;
  background: var(--surface, #fff); color: var(--muted, #5d6677); border-bottom: 1px solid var(--border, #dde1e8); }
.musicus-site-bar a { color: var(--accent, #2f5bd3); text-decoration: none; font-weight: 600; }
.musicus-site-bar a:hover { text-decoration: underline; }
.musicus-site-bar .musicus-site-version { color: var(--ink, #1d2433); font-weight: 600; }
.musicus-site-bar.test { background: #fff4e0; color: #7a3e00; border-bottom-color: #f5b453; }
.musicus-site-bar.test .musicus-site-version { color: #7a3e00; }
@media (prefers-color-scheme: dark) { .musicus-site-bar.test { background: #3a2a10; color: #f5d39a; border-bottom-color: #7a5a20; }
  .musicus-site-bar.test .musicus-site-version { color: #f5d39a; } }
@media print { .musicus-site-bar { display: none; } }
</style>"""


def bar(v: dict, latest: bool) -> str:
    e = html.escape
    if v["kind"] == "test":
        what = (f'Test build of <span class="musicus-site-version">{e(v["label"])}</span> '
                f'({e(v["commit"][:7])}) &ndash; not a released version')
    else:
        what = f'User guide for Musicus <span class="musicus-site-version">{e(v["label"])}</span>'
        if latest:
            what += " (latest release)"
    pdf = f'<a href="{e(v["pdf"])}">PDF</a>' if v["pdf"] else ""
    cls = "musicus-site-bar test" if v["kind"] == "test" else "musicus-site-bar"
    return (f'{BAR_CSS}<div class="{cls}" role="navigation" aria-label="Versions">'
            f'<a href="../">&larr; All versions</a><span>{what}</span>{pdf}</div>')


def install(v: dict, target: Path, latest: bool) -> None:
    raw = (out / v["dir"] / "guide.raw.html").read_text(encoding="utf-8")
    i = raw.find("<body")
    j = raw.find(">", i) + 1 if i >= 0 else 0  # no <body>: put it first
    target.mkdir(parents=True, exist_ok=True)
    (target / "index.html").write_text(raw[:j] + bar(v, latest) + raw[j:], encoding="utf-8")


for v in versions:
    install(v, out / v["dir"], latest=bool(releases) and v is releases[0])
if releases:
    newest = releases[0]
    install(newest, out / "latest", latest=True)
    if newest["pdf"]:
        shutil.copy(out / newest["dir"] / newest["pdf"], out / "latest" / newest["pdf"])
for v in versions:
    (out / v["dir"] / "guide.raw.html").unlink()


def row(v: dict, latest: bool) -> str:
    e = html.escape
    tag = ('<span class="badge test">test build</span>' if v["kind"] == "test"
           else '<span class="badge latest">latest</span>' if latest else "")
    href = "latest/" if latest else f'{e(v["dir"])}/'
    links = f'<a href="{href}">HTML</a>'
    if v["pdf"]:
        links += f' <a href="{href}{e(v["pdf"])}">PDF</a>'
    return (f'<li><a class="name" href="{href}">{e(v["label"])}</a>{tag}'
            f'<span class="meta">{e(v["date"])} &middot; guide {e(v["guide_commit"][:7])}</span>'
            f'<span class="links">{links}</span></li>')


tests = [v for v in versions if v["kind"] == "test"]
parts = []
if tests:
    parts.append('<h2>Test builds</h2><p class="hint">Built by hand for testing; removed by the next release.</p>'
                 f'<ul>{"".join(row(v, False) for v in tests)}</ul>')
parts.append("<h2>Releases</h2>")
if releases:
    parts.append(f'<ul>{"".join(row(v, i == 0) for i, v in enumerate(releases))}</ul>')
else:
    parts.append('<p class="hint">No released version yet.</p>')

INDEX = """<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Musicus user guide</title>
<style>
:root { --accent: #2f5bd3; --ink: #1d2433; --muted: #5d6677; --bg: #f6f7f9; --surface: #fff; --border: #dde1e8;
  --test-bg: #fff4e0; --test-ink: #7a3e00; --latest-bg: #e6f4ea; --latest-ink: #1a7f37; color-scheme: light; }
@media (prefers-color-scheme: dark) { :root { --accent: #8fb0ff; --ink: #e6e9ee; --muted: #a2abba; --bg: #12151a;
  --surface: #1a1e25; --border: #2e3540; --test-bg: #3a2a10; --test-ink: #f5d39a; --latest-bg: #16301f;
  --latest-ink: #6fd58c; color-scheme: dark; } }
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--ink);
  font: 17px/1.6 "Inter", system-ui, -apple-system, "Segoe UI", Roboto, sans-serif; }
main { max-width: 44rem; margin: 0 auto; padding: 3rem 16px 4rem; }
h1 { font-size: 2.2rem; margin: 0 0 0.3rem; letter-spacing: -0.02em; }
h1 span { color: var(--accent); }
.lead { color: var(--muted); margin: 0 0 2rem; }
h2 { font-size: 1.1rem; text-transform: uppercase; letter-spacing: 0.06em; color: var(--muted); margin: 2rem 0 0.6rem; }
.hint { color: var(--muted); font-size: 0.9rem; margin: -0.3rem 0 0.6rem; }
ul { list-style: none; padding: 0; margin: 0; background: var(--surface); border: 1px solid var(--border); border-radius: 10px; }
li { display: flex; flex-wrap: wrap; align-items: baseline; gap: 0.2rem 0.8rem; padding: 0.75rem 1rem; }
li + li { border-top: 1px solid var(--border); }
a { color: var(--accent); }
.name { font-weight: 700; font-size: 1.1rem; text-decoration: none; overflow-wrap: anywhere; }
.badge { font-size: 0.75rem; font-weight: 700; text-transform: uppercase; letter-spacing: 0.05em; padding: 0.1rem 0.5rem; border-radius: 999px; }
.badge.latest { background: var(--latest-bg); color: var(--latest-ink); }
.badge.test { background: var(--test-bg); color: var(--test-ink); }
.meta { color: var(--muted); font-size: 0.85rem; }
.links { margin-left: auto; display: flex; gap: 0.8rem; font-size: 0.95rem; }
footer { margin-top: 3rem; padding-top: 1rem; border-top: 1px solid var(--border); color: var(--muted); font-size: 0.85rem; }
</style></head>
<body><main>
<h1><span>&#x1D11E;</span> Musicus user guide</h1>
<p class="lead">Installation and use of Musicus, for every released version.</p>
%s
<footer>The user guide is licensed under the
<a href="https://creativecommons.org/licenses/by-sa/4.0/">Creative Commons Attribution-ShareAlike 4.0 International License (CC BY-SA 4.0)</a>.
Musicus itself is free software, licensed under the
<a href="https://www.gnu.org/licenses/agpl-3.0.html">GNU Affero General Public License, version 3 (AGPL-3.0)</a>.
Source code: <a href="https://github.com/Musicus-Sheets/Musicus">github.com/Musicus-Sheets/Musicus</a>.</footer>
</main></body></html>
"""
(out / "index.html").write_text(INDEX % "\n".join(parts), encoding="utf-8")
(out / "versions.json").write_text(json.dumps(
    {"latest": releases[0]["label"] if releases else None, "versions": versions}, indent=2) + "\n", encoding="utf-8")
print(f"site: {len(releases)} release(s), {len(tests)} test build(s) -> {out}")
PY
