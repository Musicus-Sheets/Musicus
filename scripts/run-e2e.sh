#!/usr/bin/env bash
# Runs one end-to-end browser test (e2e/*.mjs) against a freshly started server, the same way
# locally and in CI (workflow `E2E`, .github/workflows/e2e.yml).
#
# Usage: scripts/run-e2e.sh <smoke|smoke-firefox|german|pseudo|outage> [out-dir]
#
# Needs: the server program and the built web client, e2e dependencies installed
# (cd e2e && npm ci && npx playwright install chromium [firefox]).
#   MUSICUS_SERVER  server program   (default target/release/musicus-server)
#   MUSICUS_WEB     web client folder (default target/dx/musicus-web/release/web/public)
#   E2E_PORT        port on 127.0.0.1 (default 8191; outage also uses port+1)
# out-dir (default target/e2e/<test>): screenshots, the server log, the sample PDF.
#
# smoke, german and pseudo each get a fresh data directory and the one-time admin invite link
# from the server log; outage starts and stops its own server. The server is stopped when the
# test ends, whatever happens.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
test="${1:-}"
case "$test" in
  smoke | smoke-firefox | german | pseudo | outage) ;;
  *) echo "usage: scripts/run-e2e.sh <smoke|smoke-firefox|german|pseudo|outage> [out-dir]" >&2; exit 2 ;;
esac
out="${2:-$ROOT/target/e2e/$test}"
server="${MUSICUS_SERVER:-$ROOT/target/release/musicus-server}"
web="${MUSICUS_WEB:-$ROOT/target/dx/musicus-web/release/web/public}"
port="${E2E_PORT:-8191}"

[[ -x "$server" ]] || { echo "no server program at $server (set MUSICUS_SERVER)" >&2; exit 1; }
[[ -f "$web/index.html" ]] || { echo "no web client at $web (set MUSICUS_WEB)" >&2; exit 1; }
mkdir -p "$out"
out="$(cd "$out" && pwd)"

if [[ "$test" == outage ]]; then
  exec node "$ROOT/e2e/outage.mjs" "$server" "$web" "$port" "$out"
fi

data="$(mktemp -d "${TMPDIR:-/tmp}/musicus-e2e-$test.XXXXXX")"
log="$out/server.log"
base="http://127.0.0.1:$port"
pid=""
cleanup() {
  if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
    kill -TERM "$pid" 2>/dev/null || true
    for _ in $(seq 1 100); do kill -0 "$pid" 2>/dev/null || break; sleep 0.1; done
    kill -KILL "$pid" 2>/dev/null || true
  fi
  rm -rf "$data"
}
trap cleanup EXIT

NO_COLOR=1 RUST_LOG="${RUST_LOG:-info}" "$server" serve --data-dir "$data" --web-dir "$web" \
  --listen "127.0.0.1:$port" --public-url "$base" --insecure-cookies >"$log" 2>&1 &
pid=$!

# The first start creates `admin` and logs its one-time link, then "ready".
invite=""
for _ in $(seq 1 600); do
  kill -0 "$pid" 2>/dev/null || { echo "the server stopped:" >&2; cat "$log" >&2; exit 1; }
  if [[ -z "$invite" ]]; then
    invite="$(grep -o "$base/invite/[A-Za-z0-9_-]*" "$log" | head -n 1 || true)"
  fi
  if [[ -n "$invite" ]] && grep -qw ready "$log"; then break; fi
  sleep 0.1
done
[[ -n "$invite" ]] && grep -qw ready "$log" || { echo "the server did not get ready:" >&2; cat "$log" >&2; exit 1; }
echo "server ready on $base (log: $log)"

case "$test" in
  smoke)
    node "$ROOT/e2e/sample-pdf.mjs" "$out/sample.pdf"
    node "$ROOT/e2e/smoke.mjs" "$invite" "$out/sample.pdf" "$out/shots"
    ;;
  smoke-firefox)
    node "$ROOT/e2e/sample-pdf.mjs" "$out/sample.pdf"
    BROWSER=firefox node "$ROOT/e2e/smoke.mjs" "$invite" "$out/sample.pdf" "$out/shots"
    ;;
  german) node "$ROOT/e2e/german.mjs" "$invite" "$out/shots" ;;
  pseudo) node "$ROOT/e2e/pseudo.mjs" "$invite" ;;
esac
