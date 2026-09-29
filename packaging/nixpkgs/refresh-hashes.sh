#!/usr/bin/env bash
#
# Refreshes packaging/nixpkgs/rodeo/package.nix's `version`, `src.hash` and
# `cargoHash` for a new rodeo release — by pointing nixpkgs-unstable's
# `callPackage` at the file with both hashes blanked out to `lib.fakeHash`,
# and reading the real values back out of the two hash-mismatch errors that
# produces. This is the same trick used to derive the hashes originally
# committed in this file (see packaging/nixpkgs/README.md) and the one
# `nix-update` automates against a real nixpkgs checkout — this version
# needs no nixpkgs checkout at all, just this repo's own staged copy of the
# package expression, so it is safe to run from CI (see the `nixpkgs-staged`
# job in .github/workflows/release.yml).
#
# The GitHub release tag (`v$VERSION`) must already exist and be publicly
# fetchable before running this — it downloads that tag's source tarball to
# compute `src.hash`.
#
# Usage:
#   VERSION=0.7.0 ./packaging/nixpkgs/refresh-hashes.sh
#
# Requires: nix, with the "nix-command" experimental feature enabled
# (already the default on recent Nix, and what `cachix/install-nix-action`
# sets up in CI).
#
# Deliberately does NOT touch a nixpkgs checkout or open anything anywhere —
# see packaging/nixpkgs/publish-to-fork.sh for the (manual, human-reviewed)
# step that actually updates a nixpkgs fork and prepares a PR.

set -euo pipefail

VERSION="${VERSION:?set VERSION, e.g. VERSION=0.7.0 ./packaging/nixpkgs/refresh-hashes.sh}"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$here"

FILE="packaging/nixpkgs/rodeo/package.nix"
FAKE_HASH="sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="

nix_expr() {
  cat <<NIX
let
  pkgs = import (builtins.fetchTarball "https://github.com/NixOS/nixpkgs/archive/nixos-unstable.tar.gz") { };
in
pkgs.callPackage $here/$FILE { }
NIX
}

# Runs the build and extracts the "got: sha256-..." value from a failed
# fixed-output derivation. Fails loudly — rather than silently leaving
# lib.fakeHash in place — if the build unexpectedly succeeds (a real hash
# was already there) or Nix's error text doesn't match the expected shape.
build_and_capture_hash() {
  local log
  if log="$(nix build --impure --no-link --expr "$(nix_expr)" -L 2>&1)"; then
    echo "error: expected a hash mismatch (fakeHash still in place) but the build succeeded" >&2
    return 1
  fi
  local hash
  hash="$(echo "$log" | grep -oP 'got:\s+\Ksha256-\S+' | tail -n1)"
  if [ -z "$hash" ]; then
    echo "error: could not find a 'got: sha256-...' line in the build output:" >&2
    echo "$log" >&2
    return 1
  fi
  echo "$hash"
}

# sha256-in-SRI-base64 values can contain '/' and '+', so every
# substitution that inserts one uses '#' as the sed 's' delimiter instead
# of the default '/' ('#' never appears in an SRI hash or in $VERSION). The
# "0,/…/" *address* below still uses '/' — that part is a fixed literal
# regex with no hash in it, so it stays safe as-is; only the trailing 's'
# command's delimiter changes.

echo "==> Bumping version to $VERSION"
sed -i "s#version = \"[^\"]*\"#version = \"$VERSION\"#" "$FILE"

echo "==> Resetting src.hash and cargoHash to fakeHash"
# Case-sensitive: "hash = " (lowercase) only matches the src.hash field, not
# cargoHash (capital H) — and the "0,/…/" address limits it to the first
# match in the file, so it never touches the second occurrence.
sed -i "0,/hash = \"sha256-[^\"]*\"/s#hash = \"sha256-[^\"]*\"#hash = \"$FAKE_HASH\"#" "$FILE"
sed -i "s#cargoHash = \"sha256-[^\"]*\"#cargoHash = \"$FAKE_HASH\"#" "$FILE"

echo "==> Building with fakeHash to learn the real src.hash (tag v$VERSION must exist on GitHub already)"
src_hash="$(build_and_capture_hash)"
sed -i "0,/hash = \"$FAKE_HASH\"/s#hash = \"$FAKE_HASH\"#hash = \"$src_hash\"#" "$FILE"
echo "    src.hash = $src_hash"

echo "==> Building again to learn the real cargoHash"
cargo_hash="$(build_and_capture_hash)"
sed -i "s#cargoHash = \"$FAKE_HASH\"#cargoHash = \"$cargo_hash\"#" "$FILE"
echo "    cargoHash = $cargo_hash"

echo "==> Final build to confirm both hashes are correct"
nix build --impure --no-link --expr "$(nix_expr)" -L

echo "==> Done. $FILE is now at version $VERSION with verified hashes."
echo "    Next: packaging/nixpkgs/publish-to-fork.sh to push this into a nixpkgs checkout."
