# Debian / Ubuntu (.deb) packaging

`build.sh` + `control.in` build a `rodeo_<version>_amd64.deb` from the
prebuilt `x86_64-unknown-linux-gnu` release asset — the same one
`packaging/aur/rodeo-bin` and `packaging/rpm` use. Verified locally with
`dpkg-deb` (via `nix shell nixpkgs#dpkg`): builds cleanly, correct control
metadata, binary/man page/themes/docs laid out as expected.

No package named `rodeo` exists in Debian or Ubuntu's archives today, so
the name itself isn't a blocker, unlike the AUR.

`.github/workflows/release.yml` now runs this script automatically (with
`SRC_DIR` pointed at the tree it just built, skipping the download — see
the `Build .deb and .rpm` step) and attaches the resulting `.deb` to every
GitHub release, the same way it already does for the `.tar.gz`. Running it
yourself, below, is for local testing or building an older/unpublished
version, not required to get a `.deb` out of a release anymore.

```sh
VERSION=0.6.0 ./packaging/deb/build.sh
# -> packaging/deb/rodeo_0.6.0_amd64.deb
sudo apt install ./packaging/deb/rodeo_0.6.0_amd64.deb
```

## Distribution options

Debian/Ubuntu have no self-service community repo equivalent to the AUR or
Fedora Copr. Realistic options, roughly in order of effort:

1. **Attach the `.deb` to GitHub releases directly** — same as the existing
   `.tar.gz` asset; users `dpkg -i` / `apt install ./rodeo_*.deb`. Zero
   process, matches what `build.sh` already produces.
2. **Self-hosted apt repository** — e.g. a small `Packages`/`Release` tree
   served from GitHub Pages, added with `add-apt-repository`. More setup
   (repo signing with a GPG key, index generation on each release) but
   gives users `apt install rodeo` with update tracking, closer to the AUR
   experience.
3. **Official Debian archive** — by far the largest undertaking: file an
   "Intent to Package" (ITP) bug against `wnpp`, package to Debian Policy
   (proper `debian/` source packaging via debhelper, DEP-5
   `debian/copyright`, lintian-clean), and find a Debian Developer sponsor
   to review and upload it — sponsorship requests go through
   `mentors.debian.net`/`debian-mentors`. Not attempted here; `control.in`'s
   copyright handling (a straight copy of `LICENSE`) is deliberately
   informal and wouldn't pass Debian's stricter format requirements as-is.

## Caveat: minimum glibc version

Same as the RPM package — the release binary is built on GitHub's
`ubuntu-latest` runner and its `libc.so.6` requirement is `GLIBC_2.39`.
Fine for current Debian/Ubuntu, but too new for e.g. Debian 12 (glibc 2.36)
or Ubuntu 22.04 (glibc 2.35). Worth flagging in any release notes if this
ships.

## Before distributing

```sh
VERSION=0.6.0 ./packaging/deb/build.sh
lintian packaging/deb/rodeo_0.6.0_amd64.deb
```

(`lintian` wasn't available to test with in this environment — run it
before publishing; expect at least a complaint about the informal
`copyright` file mentioned above.)
