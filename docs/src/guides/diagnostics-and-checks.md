# Diagnostics and Checks

FigureFit separates layout solving from quality policy.

## Quality Gates

```toml
[quality]
min_height_utilization = 0.85
max_void_frac = 0.15
max_soft_violations = 0
```

With `--check`, failed quality gates produce a non-zero exit status.

## Explanations

Use:

```bash
figurefit CF01.toml --explain --top-n 5
```

The report includes candidate scores and reasons that help explain dropped groups, soft constraint penalties, and space usage.

## Determinism

Diagnostics are intended to be deterministic and actionable. Keep specs explicit, prefer stable panel IDs, and use constraints sparingly enough that failures point to a real layout conflict.
