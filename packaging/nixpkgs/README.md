# nixpkgs / NixOS packaging

Two different things live under `packaging/nixpkgs/` and the repo root,
because Nix packaging splits into two genuinely different use cases:

## `/flake.nix` (repo root) — use it today, no submission needed

Builds rodeo straight from this checkout with
`rustPlatform.buildRustPackage`, pointed at the committed `Cargo.lock` via
`cargoLock.lockFile` (no vendor hash to compute or keep in sync — Nix
trusts the lockfile's own checksums). Verified locally with `nix build`:
compiles clean, `./result/bin/rodeo --version` prints `rodeo 0.6.0`, man
page and themes installed under `share/`.

```sh
nix build github:lordgreg/rodeo        # or `nix run`, once pushed
nix develop                            # dev shell with cargo/rustc/clippy
```

This needs nothing from anyone — no account, no review, works the moment
it's pushed to the repo. It's the rodeo equivalent of `install.sh` /
Homebrew tap for Nix users, and is the thing to point people at first.

`.github/workflows/ci.yml` now builds this flake's `default` package and
runs `nix flake check` on every push/PR, so a change that breaks the flake
(a stale `cargoLock.lockFile` reference, a removed file `postInstall`
relies on) fails CI the same way a broken `cargo build` would.

## `nixpkgs/rodeo/package.nix` — staged for an upstream nixpkgs PR

The same package, but shaped the way nixpkgs itself expects: source fetched
via `fetchFromGitHub` pinned to the `v0.6.0` tag (not the local checkout),
with an explicit `cargoHash` for the vendored dependencies (nixpkgs doesn't
trust a lockfile path the way a local flake can). Both hashes in the file
are real, computed by building against nixpkgs-unstable locally:

- `src.hash` — the tag's source tarball hash
- `cargoHash` — the vendored `Cargo.lock` dependency hash

Checked: no package named `rodeo` exists in nixpkgs today, so — unlike the
AUR — naming isn't a blocker. Submitting this only takes a GitHub PR
against `NixOS/nixpkgs`, no account approval gate the way AUR currently
has:

```sh
cp packaging/nixpkgs/rodeo/package.nix nixpkgs/pkgs/by-name/ro/rodeo/package.nix
cd nixpkgs && git checkout -b add-rodeo
git add pkgs/by-name/ro/rodeo/package.nix
git commit -m "rodeo: init at 0.6.0"
# open a PR; nixpkgs CI + a maintainer review from there
```

Both `hash` and `cargoHash` need bumping on every rodeo release (a fresh
tag changes the source hash, and any dependency bump changes the vendor
hash) — `nix-build` will report the correct value on a mismatch if
`lib.fakeHash` is swapped in first, the same way they were derived here.

## Caveat: two tests skipped in the sandbox

Both `package.nix` and `flake.nix` skip
`config::tests::a_start_directory_that_no_longer_exists_falls_back_to_home`
and `updater::tests::macos_new_version_available` during `checkPhase`. Both
are sandbox artifacts, not real bugs: Nix's build sandbox sets `$HOME` to a
nonexistent `/homeless-shelter` path (deliberately, to catch impure
builds), which the "falls back to home" test doesn't expect; the macOS
updater test assumes a context this sandbox doesn't provide. `cargo test
--locked --all` in `.github/workflows/ci.yml` already covers both on a
normal host.
