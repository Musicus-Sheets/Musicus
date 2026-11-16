#!/bin/sh
# Runs system-detect over every PDF in a directory (first MAXPAGES pages each).
# Usage: scripts/batch.sh <pdf-dir> <out-dir> [MAXPAGES=12]
# Output: <out-dir>/<pdf name without .pdf>/{result.json,pNNN.png}
# Never commit the output: overlays are derived from (possibly copyrighted) scores.
set -eu
dir=$1; out=$2; max=${3:-12}
bin=$(dirname "$0")/../target/release/system-detect
for f in "$dir"/*.pdf; do
  name=$(basename "$f" .pdf)
  echo "== $name"
  "$bin" "$f" --pages 1-"$max" --out "$out/$name" 2>&1 | tail -1
done
