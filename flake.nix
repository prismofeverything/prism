{
  description = "Prism — Milner-style process bigraphs for composable multi-scale simulation";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, fenix, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };

        # Latest stable Rust + rust-analyzer, combined into one derivation.
        # Bump by `nix flake update fenix` (or `nix flake update` for all).
        rustToolchain = fenix.packages.${system}.combine [
          fenix.packages.${system}.stable.toolchain
          fenix.packages.${system}.rust-analyzer
        ];
      in {
        devShells.default = pkgs.mkShell {
          packages = [
            rustToolchain

            # Native build deps. Add here as crates demand.
            pkgs.pkg-config
            pkgs.cmake          # highs-sys, others
            pkgs.openssl        # common transitive (-sys crates)
          ];

          # highs-sys: link against nixpkgs' HiGHS instead of building from source.
          # Comment out if you'd rather have it vendor-build.
          HIGHS_DIR = "${pkgs.highs}";

          shellHook = ''
            echo "prism dev shell — $(rustc --version)"
          '';
        };

        formatter = pkgs.nixfmt-rfc-style;
      });
}
