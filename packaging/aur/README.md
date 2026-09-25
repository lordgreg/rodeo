# AUR packaging (staged, not yet pushed)

`rodeo-bin/` holds a ready-to-push AUR package for rodeo. It isn't live yet —
new AUR account registration is currently closed (Arch disabled it during a
cleanup after a wave of malicious package uploads; the AUR team has
explicitly declined to onboard accounts manually via the mailing list in the
meantime). This directory is the staging copy so nothing is lost once an
account exists — either yours, once registration reopens, or a co-maintainer
who already has one.

## Why "rodeo-bin" and not "rodeo"

The AUR already has an unrelated package named `rodeo` (a Python data-science
IDE, unmaintained since 2017). Package names are unique, so ours can't use
that name. `crates.io` has the same problem — a different, unrelated crate
already owns the name `rodeo` — so a future source-built package (e.g.
`rodeo-fm`, building from the GitHub tag with `cargo build --release
--locked`) would need its own distinct name too, separate from `rodeo-bin`.

## Before pushing, on an Arch machine

```sh
cd packaging/aur/rodeo-bin

# Regenerate .SRCINFO from PKGBUILD (must always match — the AUR rejects
# pushes where they've drifted)
makepkg --printsrcinfo > .SRCINFO

# Build and install locally to sanity-check the package
makepkg -si

# Lint for common packaging mistakes
namcap PKGBUILD
namcap *.pkg.tar.zst
```

## Publishing once an AUR account + SSH key exist

```sh
git clone ssh://aur@aur.archlinux.org/rodeo-bin.git
cp packaging/aur/rodeo-bin/{PKGBUILD,.SRCINFO} rodeo-bin/
cd rodeo-bin
git add PKGBUILD .SRCINFO
git commit -m "rodeo-bin 0.6.0"
git push
```

The first push both creates and publishes the package.

## Keeping it current

Each new rodeo release needs `pkgver`, the `source` URL, and `sha256sums`
bumped in `PKGBUILD` (mirroring the version and the matching
`.sha256` file from that release's GitHub assets), `.SRCINFO` regenerated,
then committed and pushed to the AUR repo. `.github/workflows/release.yml`
already does the equivalent for the Homebrew tap (see the `homebrew` job) —
once there's an AUR SSH deploy key to add as a repo secret, that job's
pattern (checkout the target repo, edit in place, commit, push) can be
mirrored here with a `HOMEBREW_TAP_TOKEN`-style secret.
