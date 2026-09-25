# Packages the prebuilt x86_64-unknown-linux-gnu release asset that
# .github/workflows/release.yml already publishes for every tag — binary,
# man page and themes, the same bin/share layout install.sh, the Homebrew
# formula (lordgreg/homebrew-rodeo) and the AUR package
# (packaging/aur/rodeo-bin) use. No compilation happens here.
#
# This is meant for self-hosted distribution via Copr (a FAS account and
# `copr-cli create` is all that takes — see packaging/rpm/README.md), not
# an official Fedora repo submission. Official inclusion would mean
# rebuilding from source with rust2rpm/rustPlatform-equivalent RPM macros
# per https://docs.fedoraproject.org/en-US/packaging-guidelines/Rust/ and
# going through Fedora's package review + sponsorship process — a much
# bigger commitment, left for later if ever pursued. (Checked: no package
# named "rodeo" exists in Fedora today, so the name itself is free either
# way — unlike the AUR situation.)
#
# The binary already had `strip` run on it in CI; disable rpmbuild's own
# debuginfo extraction so it doesn't try to generate a (useless, and
# possibly broken) debuginfo subpackage from an already-stripped binary.
%global debug_package %{nil}

# The upstream release only publishes a linux binary for this target.
ExclusiveArch:  x86_64

# sha256 of the Source0 tarball for the version below. Both this and
# Version get bumped together on every release — by hand for a manual
# rebuild, or by release.yml's `deb` step via sed for CI builds (it already
# has the freshly-built tarball and its checksum to hand, so it doesn't
# need to re-download and re-verify what it just built).
%global source_sha256 8c613918f4523ca6b2980d82840d524caf1b2740cad7debeadd0c85dc7f0443f

Name:           rodeo
Version:        0.6.0
Release:        1%{?dist}
Summary:        A dual-pane terminal file manager with Vim-style keybindings

License:        Apache-2.0
URL:            https://github.com/lordgreg/rodeo
Source0:        https://github.com/lordgreg/rodeo/releases/download/v%{version}/rodeo-%{version}-x86_64-unknown-linux-gnu.tar.gz

BuildArch:      x86_64

%description
Rodeo is a terminal file manager inspired by Norton and Midnight Commander,
written in Rust. It pairs the classic dual-pane layout with Vim-style
keybindings, a rich preview, and themes — with no runtime dependencies
beyond a terminal.

%prep
%setup -q -n rodeo-%{version}-x86_64-unknown-linux-gnu

# rpmbuild has no built-in per-Source0 checksum field (unlike PKGBUILD's
# sha256sums); verify by hand instead so a tampered or corrupted upstream
# asset fails the build loudly rather than getting packaged silently.
echo "%{source_sha256}  %{_sourcedir}/rodeo-%{version}-x86_64-unknown-linux-gnu.tar.gz" | sha256sum -c -

%build
# Nothing to build — packaging the prebuilt release binary as-is.

%install
install -Dm0755 rodeo %{buildroot}%{_bindir}/rodeo
install -Dm0644 rodeo.1 %{buildroot}%{_mandir}/man1/rodeo.1

# rodeo looks for its themes under $XDG_DATA_HOME/rodeo/themes at runtime.
install -d %{buildroot}%{_datadir}/rodeo/themes
install -m0644 themes/*.toml %{buildroot}%{_datadir}/rodeo/themes/

%files
%license LICENSE
%doc README.md
%{_bindir}/rodeo
%{_mandir}/man1/rodeo.1*
%{_datadir}/rodeo/themes/*.toml

%changelog
* Wed Sep 24 2025 rodeo packaging <packaging@localhost> - 0.6.0-1
- Initial package, built from the upstream x86_64-unknown-linux-gnu release asset.
