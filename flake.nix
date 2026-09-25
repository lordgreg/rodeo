{
  description = "A dual-pane terminal file manager with Vim-style keybindings";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };
        cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = cargoToml.package.name;
          version = cargoToml.package.version;

          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter =
              path: type:
              let
                base = baseNameOf path;
              in
              base != "target" && base != "flake.nix" && base != "flake.lock";
          };

          cargoLock.lockFile = ./Cargo.lock;

          # Both skipped tests are sandbox artifacts, not real failures:
          # Nix's build sandbox sets $HOME to the nonexistent
          # "/homeless-shelter" (deliberately, to catch impure builds),
          # which the "falls back to home" test doesn't expect; the macOS
          # updater test assumes a platform/fixture context this sandbox
          # doesn't provide. `cargo test --locked --all` in
          # .github/workflows/ci.yml already covers both on a normal host.
          checkFlags = [
            "--skip=config::tests::a_start_directory_that_no_longer_exists_falls_back_to_home"
            "--skip=updater::tests::macos_new_version_available"
          ];

          # rodeo looks for its themes under $XDG_DATA_HOME/rodeo/themes at
          # runtime — the same bin/share layout install.sh, the Homebrew
          # formula and the AUR package (packaging/aur/rodeo-bin) use.
          postInstall = ''
            install -Dm644 docs/rodeo.1 "$out/share/man/man1/rodeo.1"
            install -d "$out/share/rodeo/themes"
            install -m644 themes/*.toml "$out/share/rodeo/themes/"
          '';

          meta = with pkgs.lib; {
            description = "A dual-pane terminal file manager with Vim-style keybindings";
            homepage = "https://github.com/lordgreg/rodeo";
            license = licenses.asl20;
            mainProgram = "rodeo";
            platforms = platforms.unix;
          };
        };

        devShells.default = pkgs.mkShell {
          inputsFrom = [ self.packages.${system}.default ];
          packages = [ pkgs.cargo pkgs.rustc pkgs.rust-analyzer pkgs.clippy ];
        };
      }
    );
}
