+++
title = "FigureFit"

[extra]
tagline = "Scientific figure layouts that can be reviewed, checked, and reproduced."
subtitle = "FigureFit turns explicit TOML specs into manuscript-ready TikZ, stable reports, and quick SVG previews for multi-panel scientific figures."
install = "cargo install --git https://codeberg.org/caniko/FigureFit"

[[extra.features]]
title = "Constraint-Aware Layout"
description = "Encode hierarchy, required panels, optional groups, adjacency, alignment, ordering, and equal-size constraints directly in the spec."

[[extra.features]]
title = "Manuscript Outputs"
description = "Generate TikZ for LaTeX, valid TOML reports for iteration, and SVG previews for fast visual checks."

[[extra.features]]
title = "Source Aspects"
description = "Derive panel aspect ratios from SVG viewBox or width and height metadata while keeping deferred PDF/tight-bbox cases explicit."

[[extra.features]]
title = "CI Quality Gates"
description = "Use --check, --explain, and --top-n to make layout failures deterministic and actionable."

[[extra.features]]
title = "Insets"
description = "Place inset panels relative to host panels with preflight validation for invalid host references and fractions."

[[extra.features]]
title = "Compatibility"
description = "The old figure-layout binary remains as a deprecated alias while new workflows use figurefit."
+++
