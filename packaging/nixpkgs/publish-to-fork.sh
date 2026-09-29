#!/usr/bin/env bash
#
# Copies this repo's staged packaging/nixpkgs/rodeo/package.nix into a local
# nixpkgs checkout, on a fresh branch, and verifies it the same way the
# initial `add-rodeo` submission was verified (build, format check,
# structural check) — then stops, deliberately, before committing or
# pushing anything.
#
# This step is NOT run from CI and never will be: nixpkgs' own
# automation/AI policy requires "a responsible person in the loop who ...
# reviews it before submission" for every contribution, so opening or
# updating a nixpkgs PR has to stay a deliberate, human-reviewed action.
# packaging/nixpkgs/refresh-hashes.sh (which *is* safe to run unattended,
# since it only edits a file inside this repo) is what CI runs instead —
# see the `nixpkgs-staged` job in .github/workflows/release.yml.
#
# Usage:
#   NIXPKGS_DIR=/home/gregor/Projects/github/nixpkgs VERSION=0.7.0 \
#     ./packaging/nixpkgs/publish-to-fork.sh
#
# Requires: nix, nixfmt (or `nix run nixpkgs#nixfmt-rfc-style` is used as a
# fallback if it's not on PATH), and NIXPKGS_DIR pointing at an existing
# clone of your nixpkgs fork.
#
# Branches from BASE_BRANCH (default: master). Until the initial `add-rodeo`
# submission has actually merged upstream, `pkgs/by-name/ro/rodeo/` won't
# exist on master yet — in that case, point this at the pending PR branch
# instead so it keeps amending that same submission:
#   BASE_BRANCH=add-rodeo NIXPKGS_DIR=... VERSION=0.7.0 ./packaging/nixpkgs/publish-to-fork.sh

set -euo pipefail

VERSION="${VERSION:?set VERSION, e.g. VERSION=0.7.0}"
NIXPKGS_DIR="${NIXPKGS_DIR:?set NIXPKGS_DIR to your local nixpkgs fork checkout}"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
staged="$here/packaging/nixpkgs/rodeo/package.nix"
target_rel="pkgs/by-name/ro/rodeo/package.nix"

BASE_BRANCH="${BASE_BRANCH:-master}"

if [ ! -d "$NIXPKGS_DIR/.git" ]; then
  echo "error: $NIXPKGS_DIR doesn't look like a git checkout" >&2
  exit 1
fi

grep -q "version = \"$VERSION\"" "$staged" || {
  echo "error: $staged is not at version $VERSION yet." >&2
  echo "       Run: VERSION=$VERSION ./packaging/nixpkgs/refresh-hashes.sh first." >&2
  exit 1
}

cd "$NIXPKGS_DIR"

if [ -n "$(git status --porcelain)" ]; then
  echo "error: $NIXPKGS_DIR has uncommitted changes — commit, stash, or reset first." >&2
  exit 1
fi

echo "==> Syncing with $BASE_BRANCH"
current_branch="$(git rev-parse --abbrev-ref HEAD)"
git checkout "$BASE_BRANCH"
git pull --ff-only origin "$BASE_BRANCH" || echo "    (no fast-forward available for 'origin/$BASE_BRANCH' — continuing on the local branch)"

if [ ! -f "$target_rel" ]; then
  echo "error: $NIXPKGS_DIR/$target_rel doesn't exist on '$BASE_BRANCH'." >&2
  echo "       If the initial submission (packaging/nixpkgs/README.md) hasn't merged" >&2
  echo "       upstream yet, point this at that pending PR branch instead, e.g.:" >&2
  echo "           BASE_BRANCH=add-rodeo NIXPKGS_DIR=$NIXPKGS_DIR VERSION=$VERSION $0" >&2
  git checkout "$current_branch"
  exit 1
fi

branch="update-rodeo-$VERSION"
if git show-ref --verify --quiet "refs/heads/$branch"; then
  echo "error: branch $branch already exists — delete it or pick a different VERSION" >&2
  git checkout "$current_branch"
  exit 1
fi

echo "==> Creating branch $branch from $BASE_BRANCH"
git checkout -b "$branch"

echo "==> Copying staged package.nix"
cp "$staged" "$target_rel"

echo "==> Building"
nix-build -A rodeo --no-out-link

echo "==> Checking basic functionality"
result="$(nix-build -A rodeo --no-out-link)"
"$result/bin/rodeo" --version

echo "==> Formatting check"
if command -v nixfmt >/dev/null 2>&1; then
  nixfmt --check "$target_rel"
else
  nix run nixpkgs#nixfmt-rfc-style -- --check "$target_rel"
fi

echo "==> Structural check (pkgs/by-name conventions)"
./maintainers/scripts/check-by-name.sh master

cat <<EOF

==> All checks passed. $NIXPKGS_DIR is on branch '$branch' with the
    version bump staged but NOT committed — review the diff yourself:

        cd $NIXPKGS_DIR
        git diff

    Once you're satisfied, commit (add an 'Assisted-by:' trailer if any
    part of the commit message or diff was AI-drafted — see
    CONTRIBUTING.md's automation/AI policy), push, and open the PR:

        git commit -am "rodeo: <old-version> -> $VERSION"
        git push origin $branch

Previous branch was '$current_branch' — switch back with:
    git checkout $current_branch
EOF
