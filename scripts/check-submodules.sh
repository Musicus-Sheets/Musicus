#!/usr/bin/env bash
# Checks that every submodule pointer of HEAD is a commit on the first-parent
# history of the submodule's tracked branch (`branch` in .gitmodules, default
# main). That means:
#   - the docs change was merged into the docs repository's main, and
#   - the pointer names the merge commit (or a direct commit) on main, not the
#     last commit of the merged feature branch (the merge commit's 2nd parent).
# Used by .github/workflows/submodules.yml; runs locally too.
#
# Fix a failure with ./scripts/update-submodules.sh after merging the docs PRs.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# In CI there is no SSH key; the docs repositories are public, use HTTPS.
GIT_URL_OPTS=()
if [[ -n "${GITHUB_ACTIONS:-}" ]]; then
    GIT_URL_OPTS=(-c "url.https://github.com/.insteadOf=git@github.com:")
fi

failed=0
while read -r key path; do
    name="${key#submodule.}"
    name="${name%.path}"
    url="$(git config -f .gitmodules "submodule.$name.url")"
    branch="$(git config -f .gitmodules "submodule.$name.branch" || echo main)"
    pinned="$(git ls-tree HEAD "$path" | awk '{print $3}')"
    if [[ -z "$pinned" ]]; then
        echo "error: $path is not a submodule in HEAD" >&2
        failed=1
        continue
    fi

    # Full commit history, no file contents: small and fast.
    repo="$WORK/$name"
    git "${GIT_URL_OPTS[@]}" clone --quiet --bare --filter=blob:none --single-branch --branch "$branch" "$url" "$repo"

    if git -C "$repo" rev-list --first-parent "$branch" | grep -qx "$pinned"; then
        echo "ok:    $path -> ${pinned:0:12} (on $branch)"
    elif git -C "$repo" merge-base --is-ancestor "$pinned" "$branch" 2>/dev/null; then
        echo "error: $path -> ${pinned:0:12} was merged into $branch, but is not a commit of $branch itself" >&2
        echo "       (it is the last commit of the merged branch, not the merge commit)." >&2
        echo "       Run ./scripts/update-submodules.sh and commit the new pointer." >&2
        failed=1
    else
        echo "error: $path -> ${pinned:0:12} is not on $branch of $url" >&2
        echo "       Merge the docs pull request first, then run ./scripts/update-submodules.sh." >&2
        failed=1
    fi
done < <(git config -f .gitmodules --get-regexp '^submodule\..*\.path$')

exit "$failed"
