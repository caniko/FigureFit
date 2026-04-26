# FigureFit

FigureFit is a Rust solver for scientific multi-panel figure layouts. It reads a TOML layout specification, searches for a good panel tiling, and writes manuscript-ready TikZ plus a stable TOML report.

It is designed for figure iteration where panel aspect ratios, hierarchy, required groups, optional groups, insets, and quality gates need to be explicit and reviewable.

## What FigureFit Produces

- TikZ panel placement code for LaTeX manuscripts.
- A machine-readable TOML report with placed panels, dropped groups, scores, and quality checks.
- An optional SVG preview for quick visual inspection.
- Deterministic diagnostics for CI and manuscript iteration.

## Supported Today

- Explicit panel aspect ratios.
- SVG source aspect derivation from media or tight content bounds.
- PDF source aspect derivation from first-page media, crop, or tight content bounds.
- Hierarchical `atomic`, `flexible`, `one_of`, and `composite` groups.
- Insets placed relative to a host panel.
- Explicit constraints such as adjacency, alignment, ordering, and equal sizing.
- `--check`, `--explain`, `--top-n`, and `--preview`.
- Deprecated `figure-layout` compatibility binary.

## Deferred

SVG crop boxes are unsupported because SVG has no crop-box concept. PDF tight bbox derivation requires `mutool` from MuPDF. Use explicit `aspect` values when a source cannot be measured in the intended way.

Source repository: <https://codeberg.org/caniko/FigureFit>
