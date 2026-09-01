{
  description = "SharpeBench — luck-robust, forward-attested benchmark for trustworthy-with-capital AI trading agents";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };

        # The Rust version is read straight from rust-toolchain.toml so the
        # Nix build and CI/dev use one toolchain-version source of truth.
        toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

        rustPlatform = pkgs.makeRustPlatform {
          cargo = toolchain;
          rustc = toolchain;
        };
        workspaceManifest = builtins.fromTOML (builtins.readFile ./Cargo.toml);
      in
      {
        # `nix build` — an isolated build of the single `sharpebench` binary
        # (the sharpebench crate) from the committed Cargo.lock. Commit a generated
        # flake.lock before claiming the nixpkgs and rust-overlay inputs are immutable.
        packages.default = rustPlatform.buildRustPackage {
          pname = "sharpebench";
          # Cargo.toml is the version source shared by every published surface.
          version = workspaceManifest.workspace.package.version;
          src = self;

          cargoLock.lockFile = ./Cargo.lock;

          # Build/test only the CLI binary crate out of the workspace.
          cargoBuildFlags = [ "-p" "sharpebench" ];
          cargoTestFlags = [ "-p" "sharpebench" ];
          doCheck = true;

          meta = with pkgs.lib; {
            description = "Luck-robust, forward-attested AI-trading-agent benchmark CLI";
            homepage = "https://github.com/general-liquidity/sharpebench";
            license = with licenses; [ mit asl20 ];
          };
        };

        # `nix develop` — drops you into the exact pinned channel (1.96.0 +
        # rustfmt + clippy) plus cargo-deny, nothing leaking from the host.
        devShells.default = pkgs.mkShell {
          packages = [ toolchain pkgs.cargo-deny ];
          shellHook = ''
            echo "SharpeBench dev shell — $(rustc --version)"
          '';
        };

        # `nix fmt`
        formatter = pkgs.nixpkgs-fmt;
      });
}
