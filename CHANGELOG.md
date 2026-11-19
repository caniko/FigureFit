# Changelog

## 0.1.0 - Unreleased

- Initial standalone FigureFit crate.
- Provides `figurefit` CLI and deprecated `figure-layout` compatibility alias.
- Supports hierarchical TOML layout specs, panel alternatives, flexible groups,
  explicit constraints, insets, SVG source aspect derivation, preview SVGs,
  TOML reports, and CI-oriented `--check` quality gates.
- Defers PDF aspect derivation and non-media bbox source policies.
- Fixes panels shrinking in TikZ and preview output when the solved height is
  below `max_height_mm`: placement fractions are now relative to the solved
  figure height.
