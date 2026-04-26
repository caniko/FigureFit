# Command Line

```bash
figurefit <SPEC.toml> [OPTIONS]
```

Common options:

| Option | Purpose |
| --- | --- |
| `-o <dir>` | Write outputs to a directory. |
| `--check` | Fail when configured quality gates fail. |
| `--explain` | Include solver diagnostics in the report and console output. |
| `--top-n <N>` | Report the best N candidate layouts. |
| `--preview` | Write an SVG preview beside the TikZ and report outputs. |

## Deprecated Alias

```bash
figure-layout <SPEC.toml>
```

`figure-layout` remains available for compatibility and emits a deprecation warning. New scripts should use `figurefit`.

## CI Example

```bash
figurefit article/figures/layouts/CF01.toml --check --explain --top-n 3 --preview
```

Use `--check` for reproducible manuscript layout gates.
