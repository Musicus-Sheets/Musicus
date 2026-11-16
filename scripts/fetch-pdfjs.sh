#!/usr/bin/env bash
# Downloads the pinned pdf.js build (Apache-2.0) that the web client uses to
# render PDF pages, verifies its checksum and places it in
# crates/musicus-web/public/vendor/pdfjs/. Run once before building the client.
#
# We ship pdf.js' "legacy" build: the modern one relies on very new JavaScript
# APIs (e.g. Map.prototype.getOrInsertComputed) that older tablets lack.
set -euo pipefail

VERSION="6.5.438"
INTEGRITY="sha512-vg8Pw1x+AFKy4AMDvhuWl5cOYV8d698UoEANoFFhc79hqXDnZQqnOXoxw3HG907WHOg1JYOvLhP756boddFxpA=="

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="$ROOT/crates/musicus-web/public/vendor/pdfjs"
# The marker changes when the set of copied files changes, so old installs are refreshed.
MARKER="$VERSION-legacy+wasm"
if [[ -f "$TARGET/VERSION" && "$(cat "$TARGET/VERSION")" == "$MARKER" ]]; then
    echo "pdf.js $VERSION already present"
    exit 0
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
curl -fsSL "https://registry.npmjs.org/pdfjs-dist/-/pdfjs-dist-$VERSION.tgz" -o "$WORK/pdfjs.tgz"
ACTUAL="sha512-$(openssl dgst -sha512 -binary "$WORK/pdfjs.tgz" | base64 -w0)"
if [[ "$ACTUAL" != "$INTEGRITY" ]]; then
    echo "checksum mismatch for pdfjs-dist $VERSION" >&2
    exit 1
fi
tar -xzf "$WORK/pdfjs.tgz" -C "$WORK"

rm -rf "$TARGET"
mkdir -p "$TARGET"
cp "$WORK/package/legacy/build/pdf.min.mjs" "$WORK/package/legacy/build/pdf.worker.min.mjs" "$TARGET/"
cp "$WORK/package/LICENSE" "$TARGET/LICENSE"
# Image decoders for scanned sheets (JBIG2, JPEG 2000) and colour profiles; without
# them pdf.js shows such pages blank. Not copied: QuickJS (PDF scripting, unused).
mkdir -p "$TARGET/wasm"
cp "$WORK"/package/wasm/{jbig2.wasm,jbig2_nowasm_fallback.js,openjpeg.wasm,openjpeg_nowasm_fallback.js,qcms_bg.wasm} "$TARGET/wasm/"
cp "$WORK"/package/wasm/LICENSE_{JBIG2,OPENJPEG,QCMS,PDFJS_JBIG2,PDFJS_OPENJPEG,PDFJS_QCMS} "$TARGET/wasm/"
echo "$MARKER" > "$TARGET/VERSION"
echo "pdf.js $VERSION installed in $TARGET"
