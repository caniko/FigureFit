{
  description = "FigureFit: scientific multi-panel figure fitting and layout solver";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    crane,
    rust-overlay,
  }: let
    systems = ["x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin"];
    forAllSystems = f:
      nixpkgs.lib.genAttrs systems (system:
        f {
          pkgs = import nixpkgs {
            inherit system;
            overlays = [rust-overlay.overlays.default];
          };
          lib = nixpkgs.lib;
        });
  in {
    packages = forAllSystems ({
      pkgs,
      lib,
    }: let
      craneLib = crane.mkLib pkgs;
      figurefit = craneLib.buildPackage {
        src = lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions [./Cargo.toml ./Cargo.lock ./src];
        };
        strictDeps = true;
      };

      website = pkgs.stdenv.mkDerivation {
        pname = "figurefit-website";
        version = "0.1.0";
        src = lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.maybeMissing ./website;
        };
        nativeBuildInputs = [pkgs.zola];
        phases = ["buildPhase" "installPhase"];
        buildPhase = ''
          cp -r --no-preserve=mode $src/website site
          cd site
          zola build
        '';
        installPhase = ''
          cp -r public $out
        '';
      };

      docs = pkgs.stdenv.mkDerivation {
        pname = "figurefit-docs";
        version = "0.1.0";
        src = lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.maybeMissing ./docs;
        };
        nativeBuildInputs = [pkgs.mdbook];
        buildPhase = ''
          mdbook build docs
        '';
        installPhase = ''
          cp -r docs/book $out
        '';
      };

      site = pkgs.runCommand "figurefit-site" {} ''
        mkdir -p $out
        cp -r ${website}/* $out/
        mkdir -p $out/docs
        cp -r ${docs}/* $out/docs/
      '';
    in {
      inherit figurefit website docs site;
      default = site;
    });

    devShells = forAllSystems ({pkgs, ...}: {
      default = pkgs.mkShell {
        packages = let
          rustToolchain = pkgs.rust-bin.nightly.latest.default.override {
            extensions = ["rust-src" "rustfmt" "clippy"];
          };
        in [
          rustToolchain
          pkgs.mold
          pkgs.clang
          pkgs.mupdf-headless
          pkgs.zola
          pkgs.mdbook
        ];

        shellHook = ''
          echo "FigureFit development shell"
          echo "Website: cd website && zola serve"
          echo "Documentation: cd docs && mdbook serve"
        '';
      };
    });
  };
}
