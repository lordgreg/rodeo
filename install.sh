#!/usr/bin/env bash
#
# Installs rodeo on Linux from the prebuilt x86_64-unknown-linux-gnu release
# asset: binary, man page and bundled themes, laid out under one prefix
# (bin/, share/man/man1/, share/rodeo/themes/) so rodeo finds its themes on
# its own — the same bin/share layout documented in the README and used by
# the Homebrew formula.
#
# On macOS this defers to Homebrew instead — there is no macOS archive for
# this script to install by hand, and the formula already does the right
# thing (see Formula/rodeo.rb in lordgreg/homebrew-rodeo).
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/lordgreg/rodeo/master/install.sh | bash
#
# Env vars:
#   VERSION   Release to install, without the "v" prefix (default: latest)
#   PREFIX    Install prefix (default: $HOME/.local; ignored on macOS).
#             Use PREFIX=/usr/local for a system-wide, sudo install.

set -euo pipefail

REPO="lordgreg/rodeo"
PREFIX="${PREFIX:-$HOME/.local}"
VERSION="${VERSION:-}"
TARGET="x86_64-unknown-linux-gnu"

err() {
    echo "error: $*" >&2
    exit 1
}

need() {
    command -v "$1" >/dev/null 2>&1 || err "'$1' is required but not installed"
}

need curl
need tar

os="$(uname -s)"
arch="$(uname -m)"

if [ "$os" = "Darwin" ]; then
    if [ "$arch" != "arm64" ]; then
        err "the Homebrew formula only supports Apple Silicon (arm64); build from source instead, see the README"
    fi
    need brew

    tap_formula="lordgreg/rodeo/rodeo"
    # A tap added a while ago can be stale, which would install or upgrade to
    # an old version; refreshing it first keeps this in step with the latest
    # release the same way a plain `brew install` would.
    brew update >/dev/null 2>&1 || true

    if brew list --formula --versions "$tap_formula" >/dev/null 2>&1; then
        echo "rodeo is already installed via Homebrew — upgrading..."
        exec brew upgrade "$tap_formula"
    else
        echo "Installing rodeo via Homebrew..."
        exec brew install "$tap_formula"
    fi
fi

if [ "$os" != "Linux" ]; then
    err "this script supports Linux directly and macOS via Homebrew; unsupported OS '$os'"
fi

if [ "$arch" != "x86_64" ]; then
    err "only x86_64 Linux binaries are published (this machine is '$arch'); build from source instead, see the README"
fi

if [ -z "$VERSION" ]; then
    echo "Looking up the latest release..."
    VERSION="$(
        curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
            | grep -m1 '"tag_name"' \
            | sed -E 's/.*"v([^"]+)".*/\1/'
    )"
    [ -n "$VERSION" ] || err "could not determine the latest release version"
fi

TAG="v${VERSION}"
PACKAGE="rodeo-${VERSION}-${TARGET}"
BASE_URL="https://github.com/${REPO}/releases/download/${TAG}"

workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT

echo "Downloading ${PACKAGE}.tar.gz (${TAG})..."
curl -fsSL -o "$workdir/${PACKAGE}.tar.gz" "${BASE_URL}/${PACKAGE}.tar.gz" \
    || err "download failed — does ${TAG} exist for ${TARGET}? see https://github.com/${REPO}/releases"
curl -fsSL -o "$workdir/${PACKAGE}.tar.gz.sha256" "${BASE_URL}/${PACKAGE}.tar.gz.sha256"

echo "Verifying checksum..."
(cd "$workdir" && sha256sum -c "${PACKAGE}.tar.gz.sha256") >/dev/null \
    || err "checksum verification failed"

tar -xzf "$workdir/${PACKAGE}.tar.gz" -C "$workdir"
dir="$workdir/${PACKAGE}"

sudo=""
if [ "$(id -u)" -ne 0 ] && [ ! -w "$PREFIX" ]; then
    need sudo
    sudo="sudo"
fi

echo "Installing to ${PREFIX} (bin, share/man, share/rodeo/themes)..."
$sudo install -Dm755 "$dir/rodeo" "$PREFIX/bin/rodeo"
if [ -f "$dir/rodeo.1" ]; then
    $sudo install -Dm644 "$dir/rodeo.1" "$PREFIX/share/man/man1/rodeo.1"
fi
$sudo install -d "$PREFIX/share/rodeo/themes"
$sudo install -m644 "$dir"/themes/*.toml "$PREFIX/share/rodeo/themes/"

echo "Installed rodeo ${VERSION} to ${PREFIX}/bin/rodeo"

case ":$PATH:" in
    *":$PREFIX/bin:"*) ;;
    *) echo "note: ${PREFIX}/bin is not on \$PATH" ;;
esac
