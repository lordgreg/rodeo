# Fedora / RPM packaging

`rodeo.spec` packages the prebuilt `x86_64-unknown-linux-gnu` release asset
— the same one `packaging/aur/rodeo-bin` and `packaging/deb` use — into an
RPM. Verified locally with `rpmbuild` (via `nix shell nixpkgs#rpm`): builds
cleanly and produces a working `rodeo-0.6.0-1.x86_64.rpm` with a correctly
detected `Requires:` on glibc/libgcc symbol versions.

No package named `rodeo` exists in Fedora today, so — unlike the AUR — the
name itself isn't a blocker.

`.github/workflows/release.yml` now builds this spec automatically (with
`Version`/`%global source_sha256` bumped in a per-run copy via `sed`, from
the tarball it just built — see the `Build .deb and .rpm` step) and attaches
the resulting `.rpm` to every GitHub release, the same way it already does
for the `.tar.gz`. The manual steps below are for local testing or a Copr
push, not required to get an `.rpm` out of a release anymore.

## Distribution options

**Copr (self-service, recommended first step)** — Fedora's equivalent of a
personal package repo. Needs a Fedora Account System (FAS) account, which
(unlike AUR registration right now) is open:

```sh
sudo dnf install copr-cli
# https://accounts.fedoraproject.org, then https://copr.fedorainfracloud.org/api/
copr-cli create rodeo --chroot fedora-rawhide-x86_64 --chroot fedora-40-x86_64
copr-cli build rodeo packaging/rpm/rodeo.spec
```

Users then add the Copr repo (`dnf copr enable <you>/rodeo`) and `dnf
install rodeo`.

**Official Fedora repos** — a much bigger step: requires becoming a Fedora
packager (sponsor + package review), and per
<https://docs.fedoraproject.org/en-US/packaging-guidelines/Rust/>, Rust
*applications* not from crates.io still need a source-based build (not this
prebuilt-binary spec) so the build can be reproduced and audited from
source, matching how the rest of Fedora is built. Not attempted here.

## Caveat: minimum glibc version

The release binary is built on GitHub's `ubuntu-latest` runner. rpmbuild's
automatic dependency detection on this binary resolved a `Requires` on
`GLIBC_2.39` — recent Fedora releases are fine, but this rules out systems
with materially older glibc (e.g. RHEL 9 / Debian 12 ship glibc 2.34/2.36).
Worth keeping in mind if this ever needs to support those.

## Before submitting to Copr

```sh
mkdir -p rpmbuild/{SOURCES,SPECS,BUILD,RPMS,SRPMS,BUILDROOT}
curl -fsSL -o rpmbuild/SOURCES/rodeo-0.6.0-x86_64-unknown-linux-gnu.tar.gz \
  https://github.com/lordgreg/rodeo/releases/download/v0.6.0/rodeo-0.6.0-x86_64-unknown-linux-gnu.tar.gz
cp packaging/rpm/rodeo.spec rpmbuild/SPECS/

rpmbuild --define "_topdir $PWD/rpmbuild" -bb rpmbuild/SPECS/rodeo.spec
rpmlint rpmbuild/RPMS/x86_64/rodeo-*.rpm
```

(`rpmlint` wasn't available to test with in this environment — run it on a
real Fedora box before publishing.)
