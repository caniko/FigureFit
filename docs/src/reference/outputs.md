# Outputs

For an input named `CF01.toml`, FigureFit writes:

| File | Description |
| --- | --- |
| `CF01_layout.tex` | TikZ placement code. |
| `CF01_report.toml` | TOML report for automation and review. |
| `CF01_preview.svg` | Optional preview when `--preview` is enabled. |

## TikZ

The TikZ output contains positioned panel nodes sized from the solver result. It is intended to be included from LaTeX:

```tex
\input{article/figures/layouts/CF01_layout.tex}
```

## Report

The report is valid TOML and includes placed panel rectangles, dropped groups, quality status, and score details. Rectangle fractions (`x_frac`, `y_frac`, `w_frac`, `h_frac`) are relative to the solved figure, `width_mm` by `height_mm`, not to `max_height_mm`. It is stable enough to compare during manuscript iteration.

## Preview

The preview SVG is a fast visual check. Use it to catch obvious spacing and dropped-panel issues before compiling the manuscript.
