# Installation

FigureFit is a Rust command-line tool and library.

## From Source

```bash
git clone https://codeberg.org/caniko/FigureFit.git
cd FigureFit
cargo install --path .
```

This installs the canonical `figurefit` binary and the deprecated compatibility alias `figure-layout`.

## Development Shell

The repository includes a Nix flake:

```bash
nix develop
cargo test
```

The shell includes Rust, Zola, and mdBook so the solver, website, and documentation can be developed together.

## Verify The Install

```bash
figurefit --help
figure-layout --help
```

`figure-layout` prints a deprecation warning and forwards to the same implementation.
