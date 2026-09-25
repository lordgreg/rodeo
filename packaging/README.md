# Packaging

Staged, locally-verified packaging for rodeo across a few ecosystems. None
of it is published yet — see each subdirectory's README for status and
next steps.

| Path | Target | Status |
| --- | --- | --- |
| `aur/rodeo-bin/` | Arch Linux (AUR) | Ready to push; blocked on AUR account (registration currently closed, see below) |
| `nixpkgs/rodeo/package.nix` + `/flake.nix` | Nix / NixOS | `flake.nix` usable today (no gate); built + `nix flake check`ed on every CI run; nixpkgs PR ready to open |
| `rpm/rodeo.spec` | Fedora (Copr) | `.github/workflows/release.yml` builds and attaches a `.rpm` to every GitHub release automatically; publishing it to Copr still needs a (currently open) FAS account |
| `deb/` | Debian / Ubuntu (.deb) | `.github/workflows/release.yml` builds and attaches a `.deb` to every GitHub release automatically; no formal gate |

## Common thread

Every one of these (except the local `flake.nix`, which builds straight
from the checkout) repackages the same prebuilt
`rodeo-<version>-x86_64-unknown-linux-gnu.tar.gz` release asset that
`.github/workflows/release.yml` already publishes on tag — binary, man
page, themes, laid out the same way `install.sh` and the Homebrew formula
(`lordgreg/homebrew-rodeo`) install them: `bin/`, `share/man/man1/`,
`share/rodeo/themes/`. No package here compiles rodeo itself except the Nix
ones (`buildRustPackage`, using the committed `Cargo.lock`).

All of these were built and sanity-checked locally before being written up
here (`makepkg`/`rpmbuild`/`dpkg-deb`/`nix build` — see each README), not
just hand-written from convention.

## CI

`.github/workflows/release.yml`'s `build` job now builds the `.deb` and
`.rpm` as part of its existing `x86_64-unknown-linux-gnu` leg (right after
it packages the `.tar.gz`, reusing that same tree instead of re-downloading
it), and uploads them alongside the other release assets — the `release`
job's `files: assets/*` already picks up anything uploaded, no changes
needed there. `.github/workflows/ci.yml` builds the `flake.nix` package and
runs `nix flake check` on every push/PR, so a broken flake fails CI the
same way a broken `cargo build` would.

AUR and nixpkgs-PR publishing remain manual (an account gate for AUR, a
review process for nixpkgs) — see their own READMEs.

## Naming

`rodeo` is available as a package name everywhere *except* the AUR, which
already has an unrelated, unmaintained package literally called `rodeo` (a
Python data-science IDE, last touched 2017) — hence `rodeo-bin` there.
Checked directly against Fedora, Debian/Ubuntu, crates.io and nixpkgs: no
collisions, so those all use the plain `rodeo` name.

## AUR account status

New AUR account registration is currently disabled — Arch closed it during
cleanup after a wave of malicious package uploads, and has explicitly
declined to onboard accounts manually via the mailing list in the
meantime. `packaging/aur/rodeo-bin` is staged and ready; it just needs
either registration to reopen, or a co-maintainer who already has an
account, before it can be pushed.
