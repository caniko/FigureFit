# Quick Start

Create a layout spec:

```toml
[page]
width_mm = 160
max_height_mm = 216
panel_gap_mm = 2.0
group_gap_mm = 4.0

[figure]
name = "CF01"

[[panel]]
id = "A"
aspect = 1.2

[[panel]]
id = "B"
aspect = 0.8

[[group]]
id = "root"
kind = "composite"
children = ["main"]

[[group]]
id = "main"
kind = "atomic"
panels = ["A", "B"]
```

Run the solver:

```bash
figurefit CF01.toml --check --preview --explain --top-n 3
```

FigureFit writes:

- `CF01_layout.tex`
- `CF01_report.toml`
- `CF01_preview.svg` when `--preview` is set

Use `-o <dir>` to write outputs to a separate directory.
