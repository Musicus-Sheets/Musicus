#!/usr/bin/env bash
# Writes the schema snapshot crates/musicus-server/schema.sql: the structure of a new
# database after all database updates (release updates and previews), created by the real
# update runner (example `db_schema`: `Db::open` on an empty database, then
# `updates::schema::snapshot`). See a-db-check, R-OPS-038.
#
#   scripts/db-schema.sh           # regenerate the file
#   scripts/db-schema.sh --check   # fail (exit 1, with the diff) if the file is out of date
#
# Run it after adding or changing a preview update, and commit the file with the preview.
# Used by the `db-check` job of .github/workflows/database.yml.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
FILE=crates/musicus-server/schema.sql
case "${1:-}" in
    "") mode="write" ;;
    --check) mode="check" ;;
    *)
        echo "usage: $0 [--check]" >&2
        exit 2
        ;;
esac

NEW="$(mktemp)"
trap 'rm -f "$NEW"' EXIT
# A release build (MUSICUS_RELEASE_BUILD=1) refuses previews; the snapshot is of the development line.
env -u MUSICUS_RELEASE_BUILD cargo run --quiet --locked -p musicus-server --example db_schema >"$NEW"

if [[ "$mode" == write ]]; then
    if cmp -s "$NEW" "$FILE"; then
        echo "$FILE is up to date."
    else
        cp "$NEW" "$FILE"
        echo "$FILE written; commit it together with the database update."
    fi
    exit 0
fi

if [[ ! -f "$FILE" ]]; then
    echo "error: $FILE is missing. Run scripts/db-schema.sh and commit the file." >&2
    exit 1
fi
if ! diff -u --label "$FILE (committed)" --label "$FILE (from the database updates)" "$FILE" "$NEW"; then
    echo "error: $FILE does not match the database the updates create. Run scripts/db-schema.sh" \
        "and commit the file (together with the new preview update that changes the database)." >&2
    exit 1
fi
echo "$FILE is up to date."
