# Manuscript Workflow

FigureFit works best when layout specs live next to the manuscript figure sources.

1. Define panels and hierarchy in a TOML spec.
2. Run `figurefit --preview` for fast inspection.
3. Run `figurefit --check --explain --top-n 3` before committing.
4. Include the generated TikZ from LaTeX.
5. Keep the report TOML for review and regression checks.

Avoid hardcoding panel dimensions in plotting code. Let the layout spec and generated TikZ define final print dimensions.

## Source-Derived Aspects

For SVG panels, prefer:

```toml
[[panel]]
id = "A"
source = "../panels/A.svg"
```

Use `bbox = "tight"` when visible content bounds are the right layout target. PDF tight bbox derivation requires `mutool`; use explicit `aspect` values for sources that cannot be measured reliably in the development or CI environment.
