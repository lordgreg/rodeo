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
has. **Status: submitted** — live as `pkgs/by-name/ro/rodeo/package.nix`
on the `add-rodeo` branch of the `lordgreg/nixpkgs` fork, PR open against
`NixOS/nixpkgs:master`.

## Updating for a new release

Two scripts split this into a safe, fully-automatable half and a
human-review-required half — because nixpkgs' own
[automation/AI policy](https://github.com/NixOS/nixpkgs/blob/master/CONTRIBUTING.md#automationai-policy)
requires "a responsible person in the loop who ... reviews it before
submission" for every contribution, a nixpkgs PR can never be opened by
unattended CI — but keeping this repo's own staged copy current involves
no such risk, since nothing leaves this repo.

1. **`refresh-hashes.sh`** — bumps `version` and recomputes `src.hash` +
   `cargoHash` in `packaging/nixpkgs/rodeo/package.nix`, by setting both to
   `lib.fakeHash` and reading the real values back out of the resulting
   hash-mismatch errors (the same trick used to derive the hashes
   originally committed here). Touches nothing outside this repo, so it's
   safe to run unattended — `.github/workflows/release.yml`'s
   `nixpkgs-staged` job runs it automatically on every tagged release and
   commits the refreshed file straight to `master`.

   ```sh
   VERSION=0.7.0 ./packaging/nixpkgs/refresh-hashes.sh
   ```

2. **`publish-to-fork.sh`** — copies the (already-refreshed) staged file
   into a local nixpkgs checkout, on a fresh branch, and runs the same
   verification the initial submission went through (`nix-build`,
   `nixfmt --check`, `check-by-name.sh`) — then stops, deliberately, before
   committing or pushing anything, so you can review the diff yourself
   first. This step is manual by design and always will be.

   ```sh
   NIXPKGS_DIR=/home/gregor/Projects/github/nixpkgs VERSION=0.7.0 \
     ./packaging/nixpkgs/publish-to-fork.sh
   # review `git diff` in $NIXPKGS_DIR, then commit (with an `Assisted-by:`
   # trailer if AI-drafted — see the policy link above), push, and open the PR
   ```

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
