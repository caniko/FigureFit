//! # FigureFit
//!
//! A guillotine-cut layout solver for scientific multi-panel figures.
//!
//! Given a TOML specification describing panels (with target aspect ratios) and
//! a hierarchical grouping structure, the solver finds an optimal 2D tiling that
//! minimizes aspect ratio distortion while respecting group boundaries, priority
//! ordering, and proximity constraints.
//!
//! ## Quick start
//!
//! ```no_run
//! use figurefit::{config::FigureSpec, solve};
//!
//! let toml_str = std::fs::read_to_string("my_figure.toml").unwrap();
//! let spec = FigureSpec::from_toml(&toml_str).unwrap();
//!
//! // Validate the spec
//! let errors = spec.preflight();
//! assert!(errors.is_empty(), "preflight errors: {errors:?}");
//!
//! // Solve the layout
//! let layout = solve::solve(&spec);
//!
//! // Generate TikZ output
//! let tikz = figurefit::codegen::tikz::emit_tikz(&layout, &spec.page);
//! std::fs::write("my_figure_layout.tex", tikz).unwrap();
//! ```

pub mod cli;
pub mod codegen;
pub mod config;
pub mod resolve;
pub mod solve;
mod source_aspect;
pub mod tree;
