# TOML Specification

A FigureFit spec has page geometry, figure metadata, panel definitions, optional constraints, and a group hierarchy.

## Page

```toml
[page]
width_mm = 160
max_height_mm = 216
panel_gap_mm = 2.0
group_gap_mm = 4.0
```

`width_mm` and `max_height_mm` are required. Gap fields default to sensible manuscript values when omitted.

## Panels

```toml
[[panel]]
id = "A"
aspect = 1.5
required = true
min_width = 0.2
near = ["B"]
```

`aspect` is width divided by height. If `aspect` is omitted, `source` may point to an SVG or PDF:

```toml
[[panel]]
id = "A"
source = "../panels/A.svg"
bbox = "media"
```

FigureFit derives aspect from the requested bbox policy:

| Source | `media` | `crop` | `tight` |
| --- | --- | --- | --- |
| SVG | `viewBox`, or `width`/`height` fallback | Unsupported; SVG has no crop box | Visible content bounds, including stroke |
| PDF | First-page `MediaBox` | First-page `CropBox`, with `MediaBox` fallback | First page rendered through `mutool` to SVG, then measured as SVG tight bounds |

Multi-page PDFs use page 1. Failed bbox derivation is an error; specify `aspect` explicitly when a source cannot be measured in the intended way.

## Alternatives

```toml
[[panel]]
id = "B"
aspect = 1.5

  [[panel.alt]]
  label = "wide"
  aspect = 2.0
```

The solver evaluates the alternatives and picks the aspect ratio that best fits the complete figure.

## Groups

```toml
[[group]]
id = "root"
kind = "composite"
children = ["left", "right"]

[[group]]
id = "left"
kind = "atomic"
panels = ["A"]
```

Supported group kinds:

| Kind | Behavior |
| --- | --- |
| `atomic` | All panels are placed together or the group is dropped. |
| `flexible` | Places between `min_panels` and `max_panels`. |
| `one_of` | Selects one child group. |
| `composite` | Places child groups, dropping optional low-priority children when needed. |

## Insets

Insets attach a panel to a host panel using nested `[[panel.inset]]` tables on the host panel:

```toml
[[panel]]
id = "A"
aspect = 1.5

  [[panel.inset]]
  panel = "B"
  anchor = "top-right"
  w_frac = 0.35
  h_frac = 0.35
  gap_mm = 1.0

[[panel]]
id = "B"
aspect = 1.0
```

Inset dimensions are fractions of the host panel. Supported anchors are `top-left`, `top-right`, `bottom-left`, and `bottom-right`. FigureFit validates self-insets, unknown panels, duplicate inset ownership, and invalid fractions during preflight.

## Constraints

```toml
[[constraint]]
kind = "same_row"
ids = ["A", "B"]
hard = true
weight = 2.0
```

Supported kinds include `near`, `same_row`, `same_column`, `left_of`, `right_of`, `above`, `below`, alignments, `equal_width`, and `equal_height`.
