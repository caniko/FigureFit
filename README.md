# FigureFit

A scientific multi-panel figure fitting and layout solver.

Canonical repository: <https://codeberg.org/caniko/FigureFit>

Given a TOML specification describing panels (with target aspect ratios) and
a hierarchical grouping structure, the solver finds an optimal 2D tiling that
minimizes aspect ratio distortion while respecting group boundaries, priority
ordering, and proximity constraints. It outputs a TikZ file ready to
`\input` in a LaTeX manuscript.

## Quick start

```bash
figurefit my_figure.toml
```

This reads the spec, runs the solver, and writes two files next to the input:
- `my_figure_layout.tex` — TikZ figure with absolute-positioned `\node` panels
- `my_figure_report.toml` — machine-readable report (placed panels, dropped groups, scores)

Use `-o <dir>` to write outputs elsewhere. Use `--preview` for a quick SVG
preview, `--explain --top-n N` for solver diagnostics, and `--check` for CI
quality gates.

```bash
figurefit my_figure.toml -o /tmp/out
```

The old `figure-layout` binary is kept as a deprecated compatibility alias.

## Development

FigureFit is maintained as a standalone Rust crate. From the repository root:

```bash
cargo fmt
cargo test
cargo run -- path/to/layout.toml --check --preview --explain --top-n 3
```

### As a library

```rust
use figurefit::{config::FigureSpec, solve, codegen};

let spec = FigureSpec::from_toml(&std::fs::read_to_string("my_figure.toml")?)?;
assert!(spec.preflight().is_empty());

let layout = solve::solve(&spec);
let tikz = codegen::tikz::emit_tikz(&layout, &spec.page);
let report = codegen::report::emit_report(&layout);
```

## TOML specification format

A layout spec has four sections: page geometry, figure metadata, panel
definitions, and group hierarchy.

### Minimal example

```toml
[page]
width_mm = 160          # linewidth (A4 minus margins)
max_height_mm = 216     # textheight minus caption reserve
panel_gap_mm = 2.0      # gap between panels within a group
group_gap_mm = 4.0      # gap between groups (visual separation)

[figure]
name = "CF04"

# ── Panels (flat, referenced by groups) ──────────────────────

[[panel]]
id = "A"
aspect = 0.75           # width / height

[[panel]]
id = "B"
aspect = 1.5

[[panel]]
id = "C"
aspect = 1.2

# ── Groups (hierarchy, references panels by ID) ─────────────

[[group]]
id = "root"
kind = "composite"
children = ["left", "right"]

[[group]]
id = "left"
kind = "atomic"
priority = 1
panels = ["A"]

[[group]]
id = "right"
kind = "atomic"
priority = 1
panels = ["B", "C"]
```

### Page geometry

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `width_mm` | float | required | Available width (linewidth after margins) |
| `max_height_mm` | float | required | Max height (textheight minus caption) |
| `panel_gap_mm` | float | `2.0` | Gap between panels within a group |
| `group_gap_mm` | float | `4.0` | Gap between groups |

### Panel fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `id` | string | required | Unique identifier (e.g. `"A"`, `"B"`) |
| `aspect` | float | required unless `source` is set | Target aspect ratio (width / height) |
| `source` | string | — | SVG source used to derive aspect when `aspect` is omitted |
| `bbox` | enum | `media` | Source bbox policy. Only `media` is currently supported; `crop` and `tight` are deferred |
| `required` | bool | group priority based | Whether this panel is required |
| `min_width` | float | — | Min width as fraction of page width |
| `max_width` | float | — | Max width as fraction of page width |
| `min_height` | float | — | Min height as fraction of page max height |
| `near` | list | `[]` | IDs that should share an edge (panels or groups) |
| `alt` | list | `[]` | Alternative aspect ratios (see below) |
| `label_gutter_mm` | float | — | Panel-specific label gutter metadata |
| `legend_margin_mm` | float | — | Extra legend margin metadata |

#### Alternative aspect ratios

When `aspect` is omitted and `source` points to an SVG, FigureFit derives
the panel aspect from the SVG `viewBox`, or from `width`/`height` metadata
when no `viewBox` is present. PDF aspect derivation and non-media SVG bbox
policies are not implemented in this stabilization pass; specify `aspect`
explicitly for those cases.

A panel can offer multiple aspect ratios. The solver picks whichever
produces the best overall layout:

```toml
[[panel]]
id = "B"
aspect = 1.5            # primary

  [[panel.alt]]
  label = "wide"
  aspect = 2.0

  [[panel.alt]]
  label = "tall"
  aspect = 0.67
```

### Group kinds

Groups form a tree. Leaf groups contain panel IDs; branch groups contain
child group IDs.

| Kind | Behavior |
|------|----------|
| `atomic` | All panels included, or the entire group is dropped |
| `flexible` | At least `min_panels` included; rest are optional |
| `one_of` | Exactly one child group is selected (mutually exclusive) |
| `composite` | All children included; lower-priority children may be dropped |

#### Group fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `id` | string | required | Unique identifier |
| `kind` | enum | required | `atomic`, `flexible`, `one_of`, or `composite` |
| `priority` | int | `1` | Lower = more important (1 = must include) |
| `required` | bool | `priority == 1` | Whether this group must be placed |
| `panels` | list | `[]` | Panel IDs (leaf groups only) |
| `children` | list | `[]` | Child group IDs (branch groups only) |
| `near` | list | `[]` | IDs that should be adjacent (panels or groups) |
| `min_panels` | int | — | Required for `flexible` groups |
| `max_panels` | int | — | Maximum included panels for `flexible` groups |
| `direction` | enum | — | `row` or `column` split constraint for composite children |

### Solver and quality options

```toml
[solver]
mode = "auto"        # guillotine, mosaic, or auto

[quality]
min_height_utilization = 0.85
max_void_frac = 0.15
max_soft_violations = 0
```

### Explicit constraints

Legacy `near = [...]` remains supported. New specs can use normalized
constraints:

```toml
[[constraint]]
kind = "same_row"
ids = ["A", "B"]
hard = true
weight = 2.0
```

Supported kinds: `near`, `same_row`, `same_column`, `left_of`, `right_of`,
`above`, `below`, `align_top`, `align_bottom`, `align_left`, `align_right`,
`equal_width`, and `equal_height`.

### Proximity constraints (`near`)

The `near` field uses a **unified namespace**: a reference can be a panel
ID or a group ID. The solver scores adjacency by checking whether the
referenced entities share a bounding-box edge.

```toml
[[panel]]
id = "D"
near = ["queries"]      # panel D should touch the "queries" group's bbox

[[group]]
id = "analytics"
kind = "atomic"
panels = ["E", "F"]
near = ["queries"]      # analytics group bbox should touch queries group bbox
```

When a group is referenced, its bounding box is the union of all its
member panels' rectangles.

### Shared panels

A panel can appear in multiple groups. The solver tries each ownership
assignment and picks the best. Rules:

- A panel in an `atomic` group can only be in **one** atomic group
  (preflight rejects otherwise)
- A panel can be shared between `flexible` groups, or between a `flexible`
  and an `atomic` group
- The solver assigns each shared panel to exactly one group per layout
  variant

```toml
[[panel]]
id = "E"
aspect = 1.5

[[group]]
id = "queries"
kind = "flexible"
min_panels = 1
panels = ["B", "E"]      # E shared with analytics

[[group]]
id = "analytics"
kind = "flexible"
min_panels = 1
panels = ["E", "F"]      # E shared with queries
```

## Full hierarchical example

```toml
[page]
width_mm = 160
max_height_mm = 216
panel_gap_mm = 2.0
group_gap_mm = 4.0

[figure]
name = "CF04"

[[panel]]
id = "A"
aspect = 0.75

[[panel]]
id = "B"
aspect = 1.5
near = ["C"]

[[panel]]
id = "C"
aspect = 1.5

[[panel]]
id = "D"
aspect = 1.2

[[panel]]
id = "E"
aspect = 1.5
near = ["queries"]

[[panel]]
id = "F"
aspect = 1.3

[[panel]]
id = "G"
aspect = 1.3

[[group]]
id = "root"
kind = "composite"
children = ["architecture", "data"]

[[group]]
id = "architecture"
kind = "atomic"
priority = 1
panels = ["A"]

[[group]]
id = "data"
kind = "composite"
priority = 1
children = ["queries", "analytics"]
near = ["architecture"]

[[group]]
id = "queries"
kind = "atomic"
priority = 1
panels = ["B", "C", "D"]

[[group]]
id = "analytics"
kind = "atomic"
priority = 2
panels = ["E", "F", "G"]
```

## Algorithm

The solver uses a **guillotine-cut** approach: every layout is a recursive
binary split (horizontal or vertical) of a rectangle. This guarantees
gap-free, axis-aligned tilings with a natural left-to-right, top-to-bottom
reading order.

### Key design decisions

1. **Sequence partitioning DP** — panels are treated as a strictly ordered
   array. The DP partitions contiguous sub-arrays into optimal binary
   splits, memoized on `(start, end, aspect_ratio_bucket)`.

2. **Scale-invariant memoization** — the cost function depends only on
   aspect ratios, not absolute dimensions. A vertical split at ratio `r`
   transforms aspect `a` into `a*r` and `a*(1-r)`. This makes the DP
   state independent of the root bounding box size.

3. **Discretized ratios** — splits use standard scientific proportions:
   `1:3`, `1:2`, `1:1`, `2:1`, `3:1` (ratios 0.25, 0.333, 0.5, 0.667,
   0.75).

4. **Mandatory group boundaries** — the DP is constrained to split at
   group boundary positions. Within each group, all split points are
   explored freely. This keeps the search space small even for 15+ panels.

5. **Multi-objective scoring** — primary: minimize aspect ratio distortion
   (squared log-ratio). Secondary: proximity penalty for unsatisfied
   `near` constraints.

### Performance

| Panels | Groups | Solve time |
|--------|--------|-----------|
| 4 | 2 leaf + 1 root | <1ms |
| 11 | 2 leaf + 1 root | ~8ms |

## Output

### TikZ layout (`*_layout.tex`)

A self-contained `tikzpicture` environment using absolute coordinates:

```latex
\begin{tikzpicture}
  \def\fw{\linewidth}
  \def\fh{1.1707\linewidth}
  \useasboundingbox (0,0) rectangle (\fw, -\fh);

  % Panel A (architecture) — 120.0x187.3mm
  \node[anchor=north west, inner sep=0pt] at (0.0063\fw, -0.0053\fh) {%
    \includesvg[width=0.7375\fw]{CF04_A}};
  ...
\end{tikzpicture}
```

Include in your manuscript:

```latex
\begin{figure}[htbp]
  \centering
  \input{figures/layouts/CF04_layout}
  \caption{...}
\end{figure}
```

### Report (`*_report.toml`)

Machine-readable TOML with placement coordinates, actual/target aspect
ratios, which alt was chosen, and which groups were dropped:

```toml
name = "CF04"
score = 0.0600
height_mm = 187.3
height_utilization = 0.867

[[placed]]
id = "A"
group = "architecture"
x_frac = 0.0000
y_frac = 0.0000
w_frac = 0.7500
h_frac = 0.8672
aspect_actual = 0.641
aspect_target = 0.750
alt_chosen = "primary"

[[dropped]]
group = "supplementary"
priority = 3
reason = "not_selected"
panels = ["H", "I"]
```

## Preflight validation

The solver runs preflight checks before solving. Errors are reported
with clear messages:

- No ID collisions between panels and groups (unified namespace)
- No duplicate IDs
- Leaf groups (`atomic`/`flexible`) must have panels, not children
- Branch groups (`composite`/`one_of`) must have children, not panels
- All `near` references resolve to a known panel or group
- A panel cannot be in two `atomic` groups
- `flexible` groups must have valid `min_panels`
- No cycles in the group hierarchy
- Exactly one root group (not referenced as a child by anyone)
