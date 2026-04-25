use std::path::{Path, PathBuf};

use clap::Parser;

use crate::{
    codegen::{preview, report, tikz},
    config::FigureSpec,
    solve::{self, SolveOptions},
};

#[derive(Parser)]
#[command(name = "figurefit", about = "Fit scientific multi-panel figures")]
struct Cli {
    /// Path to the figure layout TOML specification.
    input: PathBuf,

    /// Output directory (defaults to parent of input file).
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Include score components, rejected candidates, and alternate summaries in the report.
    #[arg(long)]
    explain: bool,

    /// Include up to N alternate candidate summaries in the report.
    #[arg(long, default_value_t = 0)]
    top_n: usize,

    /// Write a lightweight SVG preview alongside the TikZ/report outputs.
    #[arg(long)]
    preview: bool,

    /// Exit nonzero when quality gates, hard constraints, or required content fail.
    #[arg(long)]
    check: bool,
}

pub fn run(deprecated_alias: bool) -> miette::Result<()> {
    if deprecated_alias {
        eprintln!(
            "Warning: `figure-layout` is deprecated; use `figurefit` or `cargo run -p figurefit --`."
        );
    }

    let cli = Cli::parse();
    run_cli(cli)
}

fn run_cli(cli: Cli) -> miette::Result<()> {
    let toml_str = std::fs::read_to_string(&cli.input)
        .map_err(|e| miette::miette!("Failed to read {}: {e}", cli.input.display()))?;

    let mut spec = FigureSpec::from_toml(&toml_str)
        .map_err(|e| miette::miette!("Failed to parse TOML: {e}"))?;

    if spec.figure.name.is_none() {
        spec.figure.name = cli
            .input
            .file_stem()
            .and_then(|s| s.to_str())
            .map(String::from);
    }

    let base_dir = cli.input.parent().unwrap_or_else(|| Path::new("."));
    spec.derive_content_aspects(base_dir)
        .map_err(|e| miette::miette!("{e}"))?;

    let errors = spec.preflight();
    if !errors.is_empty() {
        eprintln!("Preflight errors:");
        for e in &errors {
            eprintln!("  - {e}");
        }
        return Err(miette::miette!("{} preflight error(s)", errors.len()));
    }

    let options = SolveOptions {
        explain: cli.explain,
        top_n: if cli.explain && cli.top_n == 0 {
            5
        } else {
            cli.top_n
        },
    };
    let layout = solve::solve_with_options(&spec, &options);

    warn_about_void(&layout);

    let out_dir = cli.output.unwrap_or_else(|| {
        cli.input
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| std::path::PathBuf::from("."))
    });
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| miette::miette!("Failed to create output dir: {e}"))?;

    let tikz_path = out_dir.join(format!("{}_layout.tex", layout.figure_name));
    std::fs::write(&tikz_path, tikz::emit_tikz(&layout, &spec.page))
        .map_err(|e| miette::miette!("Failed to write TikZ: {e}"))?;

    let report_path = out_dir.join(format!("{}_report.toml", layout.figure_name));
    std::fs::write(&report_path, report::emit_report(&layout))
        .map_err(|e| miette::miette!("Failed to write report: {e}"))?;

    let preview_path = if cli.preview {
        let path = out_dir.join(format!("{}_preview.svg", layout.figure_name));
        std::fs::write(&path, preview::emit_preview_svg(&layout))
            .map_err(|e| miette::miette!("Failed to write preview: {e}"))?;
        Some(path)
    } else {
        None
    };

    print_summary(&layout, &tikz_path, &report_path, preview_path.as_deref());

    if cli.check && !layout.checks.passed {
        eprintln!("FigureFit check failed:");
        for issue in &layout.checks.issues {
            eprintln!("  - {issue}");
        }
        return Err(miette::miette!("FigureFit check failed"));
    }

    Ok(())
}

fn warn_about_void(layout: &crate::tree::SolvedLayout) {
    const VOID_WARN_THRESHOLD: f64 = 0.15;
    for p in &layout.placements {
        if p.void_frac > VOID_WARN_THRESHOLD {
            eprintln!(
                "Warning: panel {} has {:.0}% void ({}) - allocated aspect {:.2} vs target {:.2}",
                p.panel_id,
                p.void_frac * 100.0,
                p.void_side,
                p.aspect_actual,
                p.aspect_target,
            );
        }
    }
    for ip in &layout.inset_placements {
        if ip.void_frac > VOID_WARN_THRESHOLD {
            eprintln!(
                "Warning: inset {} has {:.0}% void ({}) - allocated aspect {:.2} vs target {:.2}",
                ip.panel_id,
                ip.void_frac * 100.0,
                ip.void_side,
                ip.aspect_actual,
                ip.aspect_target,
            );
        }
    }
}

fn print_summary(
    layout: &crate::tree::SolvedLayout,
    tikz_path: &Path,
    report_path: &Path,
    preview_path: Option<&Path>,
) {
    println!("Figure: {}", layout.figure_name);
    println!(
        "Height: {:.1}mm / {:.1}mm ({:.0}%)",
        layout.height_mm,
        layout.page_max_height_mm,
        layout.height_mm / layout.page_max_height_mm * 100.0
    );
    println!("Cost:   {:.4}", layout.cost);
    println!("Panels: {}", layout.placements.len());

    for p in &layout.placements {
        let alt = p
            .alt_chosen
            .as_deref()
            .map(|a| format!(" [{a}]"))
            .unwrap_or_default();
        let void_note = if p.void_frac > 0.01 {
            format!("  void {:.0}%", p.void_frac * 100.0)
        } else {
            String::new()
        };
        println!(
            "  {} ({}): {:.1}x{:.1}mm  aspect {:.2} (target {:.2}){alt}{void_note}",
            p.panel_id, p.group_id, p.w_mm, p.h_mm, p.aspect_actual, p.aspect_target,
        );
    }

    if !layout.inset_placements.is_empty() {
        println!("Insets:");
        for ip in &layout.inset_placements {
            println!(
                "  {} (in {}): {:.1}x{:.1}mm  aspect {:.2} (target {:.2})  anchor {:?}",
                ip.panel_id,
                ip.host_id,
                ip.w_mm,
                ip.h_mm,
                ip.aspect_actual,
                ip.aspect_target,
                ip.anchor,
            );
        }
    }

    if !layout.dropped_groups.is_empty() {
        println!("Dropped:");
        for d in &layout.dropped_groups {
            println!(
                "  {} (priority {}): {} - {:?}",
                d.group_id, d.priority, d.reason, d.panel_ids,
            );
        }
    }

    println!("\nOutput:");
    println!("  {}", tikz_path.display());
    println!("  {}", report_path.display());
    if let Some(path) = preview_path {
        println!("  {}", path.display());
    }
}
