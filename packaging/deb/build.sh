#!/usr/bin/env bash
#
# Builds a rodeo .deb from the prebuilt x86_64-unknown-linux-gnu release
# asset that .github/workflows/release.yml already publishes for every tag
# — binary, man page and themes, the same bin/share layout install.sh, the
# Homebrew formula (lordgreg/homebrew-rodeo), the AUR package
# (packaging/aur/rodeo-bin) and the RPM spec (packaging/rpm/rodeo.spec) use.
# No compilation happens here — this hand-builds the package tree and calls
# dpkg-deb directly rather than going through cargo-deb/debhelper, since
# there's no source build in this flow for those tools to drive.
#
# This produces a .deb for direct distribution (a GitHub release asset,
# same as the existing tar.gz, installed with `dpkg -i` / `apt install
# ./rodeo_*.deb`) — not a submission to the official Debian archive, which
# requires an ITP bug and a Debian Developer sponsor, a full source-based
# build via debhelper, and policy compliance (lintian-clean, etc.); a much
# larger undertaking than distributing a binary this way. There's also no
# public self-service equivalent to the AUR or Fedora Copr for Debian/
# Ubuntu — a self-hosted apt repository (e.g. served from GitHub Pages) is
# the next step up from this if that's ever wanted.
#
# Usage:
#   VERSION=0.6.0 ./build.sh
#
# Or, against an already-assembled package directory (same layout as the
# extracted release tarball: rodeo, rodeo.1, themes/, README.md, LICENSE)
# instead of downloading one — this is what release.yml's `deb` step does,
# reusing the tree its own `Package release` step just built rather than
# downloading the release asset back from GitHub mid-workflow, before it
# even exists:
#   VERSION=0.6.0 SRC_DIR=/path/to/rodeo-0.6.0-x86_64-unknown-linux-gnu ./build.sh
#
# Output: rodeo_<version>_amd64.deb in this directory (override with OUT_DIR).

set -euo pipefail

VERSION="${VERSION:?set VERSION, e.g. VERSION=0.6.0 ./build.sh}"
TARGET="x86_64-unknown-linux-gnu"
REPO="lordgreg/rodeo"
ARCH="amd64"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
out_dir="${OUT_DIR:-$here}"
workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT

if [ -n "${SRC_DIR:-}" ]; then
    src="$SRC_DIR"
else
    asset="rodeo-${VERSION}-${TARGET}.tar.gz"
    base_url="https://github.com/${REPO}/releases/download/v${VERSION}"

    echo "Downloading ${asset}..."
    curl -fsSL -o "$workdir/$asset" "${base_url}/${asset}"
    curl -fsSL -o "$workdir/$asset.sha256" "${base_url}/${asset}.sha256"

    echo "Verifying checksum..."
    (cd "$workdir" && sha256sum -c "${asset}.sha256")

    tar -xzf "$workdir/$asset" -C "$workdir"
    src="$workdir/rodeo-${VERSION}-${TARGET}"
fi

pkgroot="$workdir/pkgroot"
mkdir -p \
  "$pkgroot/DEBIAN" \
  "$pkgroot/usr/bin" \
  "$pkgroot/usr/share/man/man1" \
  "$pkgroot/usr/share/rodeo/themes" \
  "$pkgroot/usr/share/doc/rodeo"

install -Dm0755 "$src/rodeo" "$pkgroot/usr/bin/rodeo"
install -Dm0644 "$src/rodeo.1" "$pkgroot/usr/share/man/man1/rodeo.1"
gzip -9n "$pkgroot/usr/share/man/man1/rodeo.1"
install -m0644 "$src"/themes/*.toml "$pkgroot/usr/share/rodeo/themes/"
install -Dm0644 "$src/README.md" "$pkgroot/usr/share/doc/rodeo/README.md"
install -Dm0644 "$src/LICENSE" "$pkgroot/usr/share/doc/rodeo/copyright"

size_kb="$(du -sk "$pkgroot" --exclude=DEBIAN | cut -f1)"

sed \
  -e "s/@VERSION@/${VERSION}/" \
  -e "s/@ARCH@/${ARCH}/" \
  -e "s/@INSTALLED_SIZE@/${size_kb}/" \
  "$here/control.in" > "$pkgroot/DEBIAN/control"

echo "Building rodeo_${VERSION}_${ARCH}.deb..."
mkdir -p "$out_dir"
dpkg-deb --build --root-owner-group "$pkgroot" "$out_dir/rodeo_${VERSION}_${ARCH}.deb"

echo "Done: $out_dir/rodeo_${VERSION}_${ARCH}.deb"
