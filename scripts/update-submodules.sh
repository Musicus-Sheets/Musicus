#!/usr/bin/env bash
# Moves every submodule pointer to the newest commit of the submodule's tracked
# branch (`branch` in .gitmodules, main) and stages the new pointers.
# Run it after the docs pull requests are merged, then commit:
#   ./scripts/update-submodules.sh && git commit -m "Point submodules to merged docs" && ./scripts/check-submodules.sh
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
git submodule update --init --remote
git add $(git config -f .gitmodules --get-regexp '^submodule\..*\.path$' | awk '{print $2}')
git submodule status
echo "Pointers staged. Commit them, then verify with ./scripts/check-submodules.sh (it checks HEAD)."
