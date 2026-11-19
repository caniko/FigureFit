//! DP-based guillotine layout solver with hierarchical group support.
//!
//! Operates on [`ResolvedSequence`]s produced by the resolver. The DP
//! enforces mandatory split points at group boundaries and uses
//! scale-invariant memoization on `(start, end, aspect_bucket)`.
//!
//! # Dimension constraints
//!
//! Panels can specify `min_width`, `max_width`, and `min_height` as
//! fractions of the page dimensions. The DP tracks the top-K candidate
//! trees per cache key so that dimension constraints can be satisfied
//! even when the aspect-optimal tree violates them.
//!
//! # Proximity scoring
//!
//! After solving the geometry, a second pass scores `near` constraints
//! by checking whether referenced entities share a bounding-box edge.
//! This uses the unified namespace: a `near` target can be a panel ID
//! or a group ID, resolved via `ResolvedSequence::id_spans` to a
//! bounding rectangle (the convex hull of the entity's panels).

use std::collections::HashMap;

use crate::config::{Anchor, ConstraintKind, FigureSpec, SolverMode};
use crate::resolve::{self, ResolvedPanel, ResolvedSequence};
use crate::tree::{
    CandidateSummary, CheckSummary, ConstraintViolation, DroppedGroup, GNode, InsetPlacement,
    Placement, Rect, SPLIT_RATIOS, ScoreComponents, SolvedLayout, SplitDir,
};

// ── Aspect ratio discretization ─────────────────────────────────────

const N_BUCKETS: usize = 60;
const A_MIN: f64 = 0.15;
const A_MAX: f64 = 7.0;

fn aspect_to_bucket(a: f64) -> Option<u16> {
    if !(A_MIN..=A_MAX).contains(&a) {
        return None;
    }
    let t = (a.ln() - A_MIN.ln()) / (A_MAX.ln() - A_MIN.ln());
    let idx = (t * (N_BUCKETS - 1) as f64).round() as usize;
    Some(idx.min(N_BUCKETS - 1) as u16)
}

fn bucket_to_aspect(bucket: u16) -> f64 {
    let t = bucket as f64 / (N_BUCKETS - 1) as f64;
    (A_MIN.ln() + t * (A_MAX.ln() - A_MIN.ln())).exp()
}

// ── DP cache (top-K) ────────────────────────────────────────────────

/// How many candidate trees to track per cache key.
/// More candidates = better dimension constraint satisfaction but slower.
const TOP_K: usize = 5;

type CacheKey = (u16, u16, u16); // (start, end, aspect_bucket)

#[derive(Debug, Clone)]
struct DpEntry {
    cost: f64,
    split: Option<(SplitDir, f64, u16)>, // (dir, ratio, split_point_k)
}

/// Sorted list of top-K entries (lowest cost first).
#[derive(Debug, Clone, Default)]
struct TopK {
    entries: Vec<DpEntry>,
}

impl TopK {
    fn best(&self) -> Option<&DpEntry> {
        self.entries.first()
    }

    /// Insert an entry, keeping only the top K by cost.
    fn insert(&mut self, entry: DpEntry) {
        // Check for duplicate split configurations.
        if let Some(ref split) = entry.split {
            for existing in &self.entries {
                if let Some(ref es) = existing.split
                    && es.0 == split.0
                    && (es.1 - split.1).abs() < 1e-9
                    && es.2 == split.2
                {
                    // Same split config — keep the cheaper one.
                    if entry.cost >= existing.cost {
                        return;
                    }
                }
            }
        }

        // Find insertion point (sorted by cost ascending).
        let pos = self.entries.partition_point(|e| e.cost < entry.cost);
        if pos >= TOP_K {
            return; // worse than all existing entries
        }
        self.entries.insert(pos, entry);
        self.entries.truncate(TOP_K);
    }
}

/// Memoized DP solver with top-K tracking.
struct Solver<'a> {
    panels: &'a [ResolvedPanel],
    /// Group boundary positions — the solver MUST split here.
    boundaries: &'a [usize],
    /// Direction constraints for specific boundary positions.
    boundary_directions: &'a HashMap<usize, SplitDir>,
    cache: HashMap<CacheKey, TopK>,
}

impl<'a> Solver<'a> {
    fn new(
        panels: &'a [ResolvedPanel],
        boundaries: &'a [usize],
        boundary_directions: &'a HashMap<usize, SplitDir>,
    ) -> Self {
        Self {
            panels,
            boundaries,
            boundary_directions,
            cache: HashMap::new(),
        }
    }

    /// Run DP and return the top-K entries for (start, end, bucket).
    fn dp_topk(&mut self, start: u16, end: u16, a_bucket: u16) -> &TopK {
        let key = (start, end, a_bucket);
        if self.cache.contains_key(&key) {
            return self
                .cache
                .get(&key)
                .expect("key confirmed present by contains_key");
        }
        let result = self.dp_topk_inner(start, end, a_bucket);
        self.cache.insert(key, result);
        self.cache.get(&key).expect("just inserted above")
    }

    /// Return the best (lowest cost) entry.
    fn dp(&mut self, start: u16, end: u16, a_bucket: u16) -> Option<DpEntry> {
        self.dp_topk(start, end, a_bucket).best().cloned()
    }

    fn dp_topk_inner(&mut self, start: u16, end: u16, a_bucket: u16) -> TopK {
        let a = bucket_to_aspect(a_bucket);
        let mut topk = TopK::default();

        // Base case: single panel.
        if end - start == 1 {
            let t = &self.panels[start as usize];
            let cost = aspect_cost(a, t.aspect);
            topk.insert(DpEntry { cost, split: None });
            return topk;
        }

        // Determine which split points to try.
        let mandatory: Vec<u16> = self
            .boundaries
            .iter()
            .filter(|&&b| b > start as usize && b < end as usize)
            .map(|&b| b as u16)
            .collect();

        let split_points: Vec<u16> = if mandatory.is_empty() {
            ((start + 1)..end).collect()
        } else {
            // Direction-constrained boundaries must be tried at the outermost
            // level that contains them. If any constrained boundaries exist in
            // this range, use ONLY those as split points — this ensures the
            // constrained split happens before unconstrained ones.
            let constrained: Vec<u16> = mandatory
                .iter()
                .filter(|&&k| self.boundary_directions.contains_key(&(k as usize)))
                .copied()
                .collect();
            if constrained.is_empty() {
                mandatory
            } else {
                constrained
            }
        };

        for k in split_points {
            // If this split point has a direction constraint, only try that direction.
            let allowed_dirs: &[SplitDir] =
                if let Some(&dir) = self.boundary_directions.get(&(k as usize)) {
                    match dir {
                        SplitDir::Vertical => &[SplitDir::Vertical],
                        SplitDir::Horizontal => &[SplitDir::Horizontal],
                    }
                } else {
                    &[SplitDir::Vertical, SplitDir::Horizontal]
                };
            for &dir in allowed_dirs {
                for &ratio in SPLIT_RATIOS {
                    let (a1, a2) = match dir {
                        SplitDir::Vertical => (a * ratio, a * (1.0 - ratio)),
                        SplitDir::Horizontal => (a / ratio, a / (1.0 - ratio)),
                    };

                    let Some(b1) = aspect_to_bucket(a1) else {
                        continue;
                    };
                    let Some(b2) = aspect_to_bucket(a2) else {
                        continue;
                    };

                    // Combine top-K from children: try all pairs of
                    // left/right candidates but keep only the best K overall.
                    let left_topk = self.dp_topk(start, k, b1).clone();
                    let right_topk = self.dp_topk(k, end, b2).clone();

                    for left in &left_topk.entries {
                        for right in &right_topk.entries {
                            let total_cost = left.cost + right.cost;
                            topk.insert(DpEntry {
                                cost: total_cost,
                                split: Some((dir, ratio, k)),
                            });
                        }
                    }
                }
            }
        }

        topk
    }

    fn reconstruct(
        &mut self,
        start: u16,
        end: u16,
        a_bucket: u16,
        entry: &DpEntry,
    ) -> Option<GNode> {
        match entry.split {
            None => {
                let t = &self.panels[start as usize];
                Some(GNode::Leaf {
                    panel_idx: start as usize,
                    alt_chosen: t.alt_label.clone(),
                })
            }
            Some((dir, ratio, k)) => {
                let a = bucket_to_aspect(a_bucket);
                let (a1, a2) = match dir {
                    SplitDir::Vertical => (a * ratio, a * (1.0 - ratio)),
                    SplitDir::Horizontal => (a / ratio, a / (1.0 - ratio)),
                };
                let b1 = aspect_to_bucket(a1)?;
                let b2 = aspect_to_bucket(a2)?;

                // For each child, pick the best sub-entry.
                let left_entry = self.dp(start, k, b1)?;
                let right_entry = self.dp(k, end, b2)?;

                let first = self.reconstruct(start, k, b1, &left_entry)?;
                let second = self.reconstruct(k, end, b2, &right_entry)?;
                Some(GNode::Split {
                    dir,
                    ratio,
                    first: Box::new(first),
                    second: Box::new(second),
                })
            }
        }
    }

    /// Get all top-K entries for the root.
    fn root_topk(&mut self, n: u16, a_bucket: u16) -> Vec<DpEntry> {
        self.dp_topk(0, n, a_bucket).entries.clone()
    }
}

// ── Constraint scoring ──────────────────────────────────────────────

/// Weight for one default soft constraint violation relative to aspect cost.
const CONSTRAINT_WEIGHT: f64 = 0.05;
const OPTIONAL_DROP_PENALTY: f64 = 0.02;
const REQUIRED_DROP_PENALTY: f64 = 1_000.0;

/// Large penalty for violating hard dimension constraints (min_width, min_height, max_width).
const DIMENSION_VIOLATION_PENALTY: f64 = 10.0;

fn build_bbox_map<'a>(
    seq: &'a ResolvedSequence,
    placements: &[(usize, Rect)],
) -> HashMap<&'a str, Rect> {
    let mut bbox_map: HashMap<&str, Rect> = HashMap::new();

    for &(idx, rect) in placements {
        let pid = &seq.panels[idx].panel_id;
        bbox_map.insert(pid.as_str(), rect);
    }

    // Group bounding boxes = union of their panels' rects.
    for (id, &(start, end)) in &seq.id_spans {
        if bbox_map.contains_key(id.as_str()) {
            continue; // panel ID, already in the map
        }
        let mut union: Option<Rect> = None;
        for &(idx, rect) in placements {
            if idx >= start && idx < end {
                union = Some(match union {
                    None => rect,
                    Some(u) => Rect::union(u, rect),
                });
            }
        }
        if let Some(u) = union {
            bbox_map.insert(id.as_str(), u);
        }
    }

    bbox_map
}

fn constraint_penalty(
    spec: &FigureSpec,
    seq: &ResolvedSequence,
    placements: &[(usize, Rect)],
    page_w: f64,
    page_h: f64,
) -> (f64, Vec<ConstraintViolation>) {
    let bbox_map = build_bbox_map(seq, placements);
    let mut penalty = 0.0;
    let mut violations = Vec::new();

    for constraint in spec.normalized_constraints() {
        if constraint.ids.len() < 2 {
            continue;
        }
        let Some(first) = bbox_map.get(constraint.ids[0].as_str()) else {
            continue;
        };
        for other_id in constraint.ids.iter().skip(1) {
            let Some(other) = bbox_map.get(other_id.as_str()) else {
                continue;
            };
            if constraint_satisfied(constraint.kind, first, other, page_w, page_h) {
                continue;
            }
            let item_penalty = constraint.weight * CONSTRAINT_WEIGHT;
            penalty += item_penalty;
            violations.push(ConstraintViolation {
                kind: format!("{:?}", constraint.kind),
                ids: vec![constraint.ids[0].clone(), other_id.clone()],
                hard: constraint.hard,
                weight: constraint.weight,
                penalty: item_penalty,
            });
        }
    }

    (penalty, violations)
}

fn constraint_satisfied(
    kind: ConstraintKind,
    a: &Rect,
    b: &Rect,
    page_w: f64,
    page_h: f64,
) -> bool {
    let tol_x = 0.01 / page_w;
    let tol_y = 0.01 / page_h;
    match kind {
        ConstraintKind::Near => shares_edge(a, b, page_w, page_h),
        ConstraintKind::SameRow => a.y < b.y + b.h - tol_y && b.y < a.y + a.h - tol_y,
        ConstraintKind::SameColumn => a.x < b.x + b.w - tol_x && b.x < a.x + a.w - tol_x,
        ConstraintKind::LeftOf => a.x + a.w <= b.x + tol_x,
        ConstraintKind::RightOf => b.x + b.w <= a.x + tol_x,
        ConstraintKind::Above => a.y + a.h <= b.y + tol_y,
        ConstraintKind::Below => b.y + b.h <= a.y + tol_y,
        ConstraintKind::AlignTop => (a.y - b.y).abs() < tol_y,
        ConstraintKind::AlignBottom => ((a.y + a.h) - (b.y + b.h)).abs() < tol_y,
        ConstraintKind::AlignLeft => (a.x - b.x).abs() < tol_x,
        ConstraintKind::AlignRight => ((a.x + a.w) - (b.x + b.w)).abs() < tol_x,
        ConstraintKind::EqualWidth => (a.w - b.w).abs() < tol_x,
        ConstraintKind::EqualHeight => (a.h - b.h).abs() < tol_y,
    }
}

/// Compute penalty for panels that violate min_width, max_width, or min_height constraints.
///
/// Each constraint is checked against the fractional placement:
/// - `w_frac >= min_width` (fraction of page width)
/// - `w_frac <= max_width` (fraction of page width)
/// - `h_frac >= min_height` (fraction of page max height)
fn dimension_penalty(seq: &ResolvedSequence, placements: &[(usize, Rect)]) -> f64 {
    let mut penalty = 0.0;

    for &(idx, rect) in placements {
        let panel = &seq.panels[idx];

        if let Some(min_w) = panel.min_width_frac
            && rect.w < min_w - 1e-6
        {
            let shortfall = (min_w - rect.w) / min_w;
            penalty += DIMENSION_VIOLATION_PENALTY * (1.0 + shortfall);
        }

        if let Some(max_w) = panel.max_width_frac
            && rect.w > max_w + 1e-6
        {
            let excess = (rect.w - max_w) / max_w;
            penalty += DIMENSION_VIOLATION_PENALTY * (1.0 + excess);
        }

        if let Some(min_h) = panel.min_height_frac
            && rect.h < min_h - 1e-6
        {
            let shortfall = (min_h - rect.h) / min_h;
            penalty += DIMENSION_VIOLATION_PENALTY * (1.0 + shortfall);
        }
    }

    penalty
}

/// Check if two rects share an edge (within tolerance).
fn shares_edge(a: &Rect, b: &Rect, page_w: f64, page_h: f64) -> bool {
    let tol_x = 0.01 / page_w; // ~0.01mm tolerance
    let tol_y = 0.01 / page_h;

    // Shared vertical edge: a's right == b's left or vice versa,
    // and they overlap vertically.
    let a_right = a.x + a.w;
    let b_right = b.x + b.w;
    let v_overlap = a.y < b.y + b.h - tol_y && b.y < a.y + a.h - tol_y;
    let h_overlap = a.x < b.x + b.w - tol_x && b.x < a.x + a.w - tol_x;

    if v_overlap && ((a_right - b.x).abs() < tol_x || (b_right - a.x).abs() < tol_x) {
        return true;
    }

    // Shared horizontal edge.
    let a_bottom = a.y + a.h;
    let b_bottom = b.y + b.h;
    if h_overlap && ((a_bottom - b.y).abs() < tol_y || (b_bottom - a.y).abs() < tol_y) {
        return true;
    }

    false
}

// ── Public API ──────────────────────────────────────────────────────

/// Maximum number of configuration variants to explore.
const MAX_VARIANTS: usize = 500;

#[derive(Debug, Clone, Copy, Default)]
pub struct SolveOptions {
    pub explain: bool,
    pub top_n: usize,
}

/// Solve the layout for a figure specification.
pub fn solve(spec: &FigureSpec) -> SolvedLayout {
    solve_with_options(spec, &SolveOptions::default())
}

pub fn solve_with_options(spec: &FigureSpec, options: &SolveOptions) -> SolvedLayout {
    let page_w = spec.page.width_mm;
    let page_h = spec.page.max_height_mm;

    let sequences = resolve::resolve(spec, MAX_VARIANTS);

    if sequences.is_empty() {
        return empty_layout(spec, page_w, page_h, "no valid resolved sequences");
    }

    let mut candidates: Vec<CandidateEval<'_>> = Vec::new();

    for seq in &sequences {
        if seq.panels.is_empty() {
            continue;
        }
        for &mode in modes_to_try(spec.solver.mode) {
            if let Some(eval) = evaluate_sequence(spec, seq, page_w, page_h, mode) {
                candidates.push(eval);
            }
        }
    }

    candidates.sort_by(|a, b| a.cost.total_cmp(&b.cost));

    let mut summaries = Vec::new();
    for (idx, candidate) in candidates.iter().take(options.top_n).enumerate() {
        summaries.push(CandidateSummary {
            rank: idx + 1,
            cost: candidate.cost,
            height_mm: candidate.height,
            panels: candidate.seq.panels.len(),
            dropped_groups: candidate.seq.dropped_group_ids.iter().cloned().collect(),
            rejected_reason: if candidate.hard_constraint_failed {
                Some("hard_constraint_failed".into())
            } else if !candidate.seq.unsatisfied_required.is_empty()
                || !candidate.seq.unsatisfied_required_panels.is_empty()
            {
                Some("unsatisfied_required".into())
            } else {
                None
            },
        });
    }

    let best_index = candidates
        .iter()
        .position(|c| {
            !c.hard_constraint_failed
                && c.seq.unsatisfied_required.is_empty()
                && c.seq.unsatisfied_required_panels.is_empty()
        })
        .unwrap_or(0);

    let Some(best) = candidates.get(best_index) else {
        return empty_layout(spec, page_w, page_h, "no solvable geometry");
    };

    build_layout(spec, best, summaries, options.explain || options.top_n > 0)
}

struct CandidateEval<'a> {
    tree: GNode,
    seq: &'a ResolvedSequence,
    mode: SolverMode,
    cost: f64,
    height: f64,
    score: ScoreComponents,
    constraint_violations: Vec<ConstraintViolation>,
    hard_constraint_failed: bool,
}

fn modes_to_try(mode: SolverMode) -> &'static [SolverMode] {
    match mode {
        SolverMode::Guillotine => &[SolverMode::Guillotine],
        SolverMode::Mosaic => &[SolverMode::Mosaic],
        SolverMode::Auto => &[SolverMode::Guillotine, SolverMode::Mosaic],
    }
}

fn evaluate_sequence<'a>(
    spec: &FigureSpec,
    seq: &'a ResolvedSequence,
    page_w: f64,
    page_h: f64,
    mode: SolverMode,
) -> Option<CandidateEval<'a>> {
    let (tree, aspect_cost, dimension_cost, height) = match mode {
        SolverMode::Guillotine | SolverMode::Auto => solve_sequence(seq, page_w, page_h)?,
        SolverMode::Mosaic => solve_sequence_mosaic(seq, page_w, page_h)?,
    };
    let bbox = Rect::new(0.0, 0.0, 1.0, height / page_h);
    let resolved = tree.resolve(bbox, page_w, page_h);
    let (constraint_cost, violations) = constraint_penalty(spec, seq, &resolved, page_w, page_h);
    let optional_content = seq.dropped_group_ids.len() as f64 * OPTIONAL_DROP_PENALTY;
    let required_content = (seq.unsatisfied_required.len() + seq.unsatisfied_required_panels.len())
        as f64
        * REQUIRED_DROP_PENALTY;
    let void = resolved
        .iter()
        .map(|(idx, rect)| {
            let p = &seq.panels[*idx];
            let actual = (rect.w * page_w) / (rect.h * page_h);
            let (void, _) = crate::tree::compute_void(actual, p.aspect);
            void
        })
        .sum();
    let score = ScoreComponents {
        aspect: aspect_cost,
        void,
        dimension: dimension_cost,
        constraint: constraint_cost,
        optional_content,
        required_content,
        height_utilization: height / page_h,
    };
    let hard_constraint_failed = violations.iter().any(|v| v.hard);
    let cost = aspect_cost + dimension_cost + constraint_cost + optional_content + required_content;

    Some(CandidateEval {
        tree,
        seq,
        mode,
        cost,
        height,
        score,
        constraint_violations: violations,
        hard_constraint_failed,
    })
}

fn build_layout(
    spec: &FigureSpec,
    best: &CandidateEval<'_>,
    candidate_summaries: Vec<CandidateSummary>,
    explain: bool,
) -> SolvedLayout {
    let page_w = spec.page.width_mm;
    let page_h = spec.page.max_height_mm;
    // Output rects are fractions of the solved figure (page_w × best.height),
    // the frame every codegen backend and inset placement draws into.
    let resolved = best
        .tree
        .resolve(Rect::new(0.0, 0.0, 1.0, 1.0), page_w, best.height);
    let panel_map = spec.panel_map();

    let placements: Vec<Placement> = resolved
        .iter()
        .map(|(idx, rect)| {
            let p = &best.seq.panels[*idx];
            let w_mm = rect.w * page_w;
            let h_mm = rect.h * best.height;
            let aspect_actual = w_mm / h_mm;
            let (void_frac, void_side) = crate::tree::compute_void(aspect_actual, p.aspect);
            let panel_def = panel_map.get(p.panel_id.as_str());
            Placement {
                panel_id: p.panel_id.clone(),
                group_id: p.group_id.clone(),
                rect: *rect,
                w_mm,
                h_mm,
                aspect_actual,
                aspect_target: p.aspect,
                alt_chosen: p.alt_label.clone(),
                pad_mm: panel_def.map_or(0.0, |d| d.pad_mm),
                label_gutter_mm: panel_def.and_then(|d| d.label_gutter_mm),
                legend_margin_mm: panel_def.and_then(|d| d.legend_margin_mm),
                void_frac,
                void_side,
            }
        })
        .collect();

    let dropped_groups = spec
        .groups
        .iter()
        .filter(|g| !g.panels.is_empty() && best.seq.dropped_group_ids.contains(g.id.as_str()))
        .map(|g| DroppedGroup {
            group_id: g.id.clone(),
            priority: g.priority,
            panel_ids: g.panels.clone(),
            reason: if best.seq.unsatisfied_required.contains(g.id.as_str()) {
                "unsatisfied_required"
            } else {
                "dropped_optional"
            }
            .into(),
        })
        .collect();

    let inset_placements = compute_inset_placements(spec, &placements, page_w, best.height);
    let checks = check_layout(spec, best, &placements);

    SolvedLayout {
        figure_name: spec.figure.name.clone().unwrap_or_default(),
        cost: best.cost,
        solver_mode: format!("{:?}", best.mode),
        height_mm: best.height,
        page_width_mm: page_w,
        page_max_height_mm: page_h,
        placements,
        inset_placements,
        dropped_groups,
        score: best.score.clone(),
        constraint_violations: best.constraint_violations.clone(),
        candidate_summaries: if explain {
            candidate_summaries
        } else {
            Vec::new()
        },
        checks,
    }
}

fn empty_layout(spec: &FigureSpec, page_w: f64, page_h: f64, issue: &str) -> SolvedLayout {
    SolvedLayout {
        figure_name: spec.figure.name.clone().unwrap_or_default(),
        cost: f64::INFINITY,
        solver_mode: format!("{:?}", spec.solver.mode),
        height_mm: 0.0,
        page_width_mm: page_w,
        page_max_height_mm: page_h,
        placements: Vec::new(),
        inset_placements: Vec::new(),
        dropped_groups: Vec::new(),
        score: ScoreComponents::default(),
        constraint_violations: Vec::new(),
        candidate_summaries: Vec::new(),
        checks: CheckSummary {
            passed: false,
            issues: vec![issue.into()],
        },
    }
}

fn check_layout(
    spec: &FigureSpec,
    best: &CandidateEval<'_>,
    placements: &[Placement],
) -> CheckSummary {
    let mut issues = Vec::new();
    if !best.seq.unsatisfied_required.is_empty() {
        issues.push(format!(
            "required groups were not placed: {:?}",
            best.seq.unsatisfied_required
        ));
    }
    if !best.seq.unsatisfied_required_panels.is_empty() {
        issues.push(format!(
            "required panels were not placed: {:?}",
            best.seq.unsatisfied_required_panels
        ));
    }
    for violation in &best.constraint_violations {
        if violation.hard {
            issues.push(format!(
                "hard constraint {} failed for {:?}",
                violation.kind, violation.ids
            ));
        }
    }
    if let Some(min_util) = spec.quality.min_height_utilization
        && best.height / spec.page.max_height_mm < min_util
    {
        issues.push(format!(
            "height utilization {:.3} is below {:.3}",
            best.height / spec.page.max_height_mm,
            min_util
        ));
    }
    if let Some(max_void) = spec.quality.max_void_frac {
        for p in placements {
            if p.void_frac > max_void {
                issues.push(format!(
                    "panel {} void {:.3} exceeds {:.3}",
                    p.panel_id, p.void_frac, max_void
                ));
            }
        }
    }
    if let Some(max_soft) = spec.quality.max_soft_violations {
        let soft = best
            .constraint_violations
            .iter()
            .filter(|v| !v.hard)
            .count();
        if soft > max_soft {
            issues.push(format!(
                "{soft} soft constraint violations exceed {max_soft}"
            ));
        }
    }
    CheckSummary {
        passed: issues.is_empty(),
        issues,
    }
}

/// Check whether any panel in the sequence has dimension constraints.
fn has_dimension_constraints(panels: &[ResolvedPanel]) -> bool {
    panels.iter().any(|p| {
        p.min_width_frac.is_some() || p.max_width_frac.is_some() || p.min_height_frac.is_some()
    })
}

/// Solve for a specific panel sequence with mandatory boundaries.
///
/// When dimension constraints exist, the solver evaluates all top-K
/// candidate trees at each figure height, picking the combination
/// with the lowest total cost (aspect distortion + dimension penalty).
fn solve_sequence(
    seq: &ResolvedSequence,
    page_w: f64,
    page_h: f64,
) -> Option<(GNode, f64, f64, f64)> {
    let panels = &seq.panels;
    let boundaries = &seq.group_boundaries;
    let n = panels.len();
    if n == 0 {
        return None;
    }

    let has_constraints = has_dimension_constraints(panels);
    let mut solver = Solver::new(panels, boundaries, &seq.boundary_directions);

    let min_a = page_w / page_h;
    let max_a = (page_w / estimate_min_height(panels, page_w)).min(A_MAX);

    // (bucket, entry_index, aspect_cost, dimension_cost, total_cost, height)
    let mut best: Option<(u16, usize, f64, f64, f64, f64)> = None;
    let steps = 30;
    let mut seen_buckets = [false; N_BUCKETS];

    for i in 0..=steps {
        let a = min_a + (max_a - min_a) * (i as f64 / steps as f64);
        let height = page_w / a;
        if height > page_h {
            continue;
        }
        let Some(a_bucket) = aspect_to_bucket(a) else {
            continue;
        };
        if seen_buckets[a_bucket as usize] {
            continue;
        }
        seen_buckets[a_bucket as usize] = true;

        if has_constraints {
            // Evaluate ALL top-K candidates at this height.
            let candidates = solver.root_topk(n as u16, a_bucket);
            for (ci, candidate) in candidates.iter().enumerate() {
                let tree = solver.reconstruct(0, n as u16, a_bucket, candidate);
                let dim_cost = if let Some(ref tree) = tree {
                    let bbox = Rect::new(0.0, 0.0, 1.0, height / page_h);
                    let placements = tree.resolve(bbox, page_w, page_h);
                    dimension_penalty(seq, &placements)
                } else {
                    continue;
                };
                let total = candidate.cost + dim_cost;
                if best.as_ref().is_none_or(|(_, _, _, _, bc, _)| total < *bc) {
                    best = Some((a_bucket, ci, candidate.cost, dim_cost, total, height));
                }
            }
        } else {
            // Fast path: no constraints, just use the best entry.
            let Some(entry) = solver.dp(0, n as u16, a_bucket) else {
                continue;
            };
            if best
                .as_ref()
                .is_none_or(|(_, _, _, _, bc, _)| entry.cost < *bc)
            {
                best = Some((a_bucket, 0, entry.cost, 0.0, entry.cost, height));
            }
        }
    }

    let (best_bucket, best_idx, aspect, dimension, _cost, height) = best?;
    let candidates = solver.root_topk(n as u16, best_bucket);
    let entry = candidates.get(best_idx)?;
    let tree = solver.reconstruct(0, n as u16, best_bucket, entry)?;
    Some((tree, aspect, dimension, height))
}

fn solve_sequence_mosaic(
    seq: &ResolvedSequence,
    page_w: f64,
    page_h: f64,
) -> Option<(GNode, f64, f64, f64)> {
    let n = seq.panels.len();
    if n == 0 {
        return None;
    }
    let cols = (n as f64).sqrt().ceil() as usize;
    let rows = n.div_ceil(cols);
    let height = (page_w * rows as f64 / cols as f64).min(page_h);
    let tree = build_grid_tree(0, n, cols)?;
    let bbox = Rect::new(0.0, 0.0, 1.0, height / page_h);
    let placements = tree.resolve(bbox, page_w, page_h);
    let aspect = placements
        .iter()
        .map(|(idx, rect)| aspect_cost(rect.aspect(page_w, page_h), seq.panels[*idx].aspect))
        .sum();
    let dimension = dimension_penalty(seq, &placements);
    Some((tree, aspect, dimension, height))
}

fn build_grid_tree(start: usize, end: usize, cols: usize) -> Option<GNode> {
    let n = end - start;
    if n == 0 {
        return None;
    }
    if n == 1 {
        return Some(GNode::Leaf {
            panel_idx: start,
            alt_chosen: None,
        });
    }
    if n <= cols {
        return build_equal_split_row(start, end);
    }
    let first_end = (start + cols).min(end);
    let first = build_equal_split_row(start, first_end)?;
    let rest = build_grid_tree(first_end, end, cols)?;
    Some(GNode::Split {
        dir: SplitDir::Horizontal,
        ratio: cols as f64 / n as f64,
        first: Box::new(first),
        second: Box::new(rest),
    })
}

fn build_equal_split_row(start: usize, end: usize) -> Option<GNode> {
    let n = end - start;
    if n == 0 {
        return None;
    }
    if n == 1 {
        return Some(GNode::Leaf {
            panel_idx: start,
            alt_chosen: None,
        });
    }
    let first = GNode::Leaf {
        panel_idx: start,
        alt_chosen: None,
    };
    let rest = build_equal_split_row(start + 1, end)?;
    Some(GNode::Split {
        dir: SplitDir::Vertical,
        ratio: 1.0 / n as f64,
        first: Box::new(first),
        second: Box::new(rest),
    })
}

/// Aspect ratio distortion cost (squared log-ratio).
fn aspect_cost(actual: f64, target: f64) -> f64 {
    let log_diff = actual.ln() - target.ln();
    log_diff * log_diff
}

/// Compute placements for inset panels (post-solve).
///
/// For each host panel that declares insets, compute the sub-rectangle
/// within the host's placement where the tenant panel is placed,
/// respecting the anchor corner and aspect ratio.
fn compute_inset_placements(
    spec: &FigureSpec,
    placements: &[Placement],
    page_w: f64,
    figure_h: f64,
) -> Vec<InsetPlacement> {
    let panel_map = spec.panel_map();
    let placement_map: std::collections::HashMap<&str, &Placement> = placements
        .iter()
        .map(|p| (p.panel_id.as_str(), p))
        .collect();

    let mut insets = Vec::new();

    for host_def in &spec.panels {
        let Some(host_place) = placement_map.get(host_def.id.as_str()) else {
            continue; // host was dropped
        };

        for inset_def in &host_def.insets {
            let Some(tenant_def) = panel_map.get(inset_def.panel.as_str()) else {
                continue;
            };

            // Find the group the tenant panel belongs to.
            let tenant_group = spec
                .groups
                .iter()
                .find(|g| g.panels.contains(&inset_def.panel))
                .map(|g| g.id.as_str())
                .unwrap_or("unknown");

            // Compute the available sub-rectangle within the host.
            let avail_w = host_place.rect.w * inset_def.w_frac;
            let avail_h = host_place.rect.h * inset_def.h_frac;

            // Place at the specified anchor corner.
            let (sub_x, sub_y) = match inset_def.anchor {
                Anchor::TopLeft => (host_place.rect.x, host_place.rect.y),
                Anchor::TopRight => (
                    host_place.rect.x + host_place.rect.w - avail_w,
                    host_place.rect.y,
                ),
                Anchor::BottomLeft => (
                    host_place.rect.x,
                    host_place.rect.y + host_place.rect.h - avail_h,
                ),
                Anchor::BottomRight => (
                    host_place.rect.x + host_place.rect.w - avail_w,
                    host_place.rect.y + host_place.rect.h - avail_h,
                ),
            };

            // Fit the tenant within the available rect, preserving aspect ratio.
            let avail_w_mm = avail_w * page_w;
            let avail_h_mm = avail_h * figure_h;
            let avail_aspect = avail_w_mm / avail_h_mm;
            let Some(target_aspect) = tenant_def.aspect else {
                continue;
            };

            let (fit_w_mm, fit_h_mm) = if target_aspect > avail_aspect {
                // Width-constrained: fill width, reduce height.
                (avail_w_mm, avail_w_mm / target_aspect)
            } else {
                // Height-constrained: fill height, reduce width.
                (avail_h_mm * target_aspect, avail_h_mm)
            };

            let fit_w_frac = fit_w_mm / page_w;
            let fit_h_frac = fit_h_mm / figure_h;

            // Center the fitted rect within the available slot at the anchor.
            let cx = sub_x + (avail_w - fit_w_frac) / 2.0;
            let cy = sub_y + (avail_h - fit_h_frac) / 2.0;

            let inset_aspect_actual = fit_w_mm / fit_h_mm;
            let (void_frac, void_side) =
                crate::tree::compute_void(inset_aspect_actual, target_aspect);
            insets.push(InsetPlacement {
                panel_id: inset_def.panel.clone(),
                host_id: host_def.id.clone(),
                group_id: tenant_group.to_string(),
                anchor: inset_def.anchor,
                rect: Rect::new(cx, cy, fit_w_frac, fit_h_frac),
                w_mm: fit_w_mm,
                h_mm: fit_h_mm,
                aspect_actual: inset_aspect_actual,
                aspect_target: target_aspect,
                alt_chosen: None,
                pad_mm: tenant_def.pad_mm,
                label_gutter_mm: tenant_def.label_gutter_mm,
                legend_margin_mm: tenant_def.legend_margin_mm,
                void_frac,
                void_side,
            });
        }
    }

    insets
}

/// Rough lower-bound estimate for minimum figure height.
fn estimate_min_height(panels: &[ResolvedPanel], page_w: f64) -> f64 {
    let n = panels.len() as f64;
    let avg_w = page_w / n.sqrt();
    let total_area: f64 = panels
        .iter()
        .map(|t| {
            let h = avg_w / t.aspect;
            avg_w * h
        })
        .sum();
    (total_area / page_w).max(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solve_toml(toml: &str) -> SolvedLayout {
        let spec = FigureSpec::from_toml(toml).unwrap();
        let errors = spec.preflight();
        assert!(errors.is_empty(), "preflight errors: {errors:?}");
        solve(&spec)
    }

    #[test]
    fn solve_4_panels_hierarchical() {
        let layout = solve_toml(
            r#"
[page]
width_mm = 160
max_height_mm = 216

[figure]
name = "CF_test"

[[panel]]
id = "A"
aspect = 1.0

[[panel]]
id = "B"
aspect = 1.5

[[panel]]
id = "C"
aspect = 1.2

[[panel]]
id = "D"
aspect = 0.8

[[group]]
id = "root"
kind = "composite"
children = ["left", "right"]

[[group]]
id = "left"
kind = "atomic"
panels = ["A", "B"]

[[group]]
id = "right"
kind = "atomic"
panels = ["C", "D"]
"#,
        );
        assert_eq!(layout.placements.len(), 4);
        assert!(layout.height_mm <= 216.0);
        assert!(layout.cost.is_finite());

        for p in &layout.placements {
            eprintln!(
                "  {} ({}): {:.1}×{:.1}mm  aspect {:.2} (target {:.2})",
                p.panel_id, p.group_id, p.w_mm, p.h_mm, p.aspect_actual, p.aspect_target,
            );
        }
    }

    #[test]
    fn solve_11_panels_under_100ms() {
        let toml = r#"
[page]
width_mm = 160
max_height_mm = 216

[figure]
name = "CF_big"

[[panel]]
id = "A"
aspect = 1.0
[[panel]]
id = "B"
aspect = 1.5
[[panel]]
id = "C"
aspect = 0.8
[[panel]]
id = "D"
aspect = 2.0
[[panel]]
id = "E"
aspect = 1.2
[[panel]]
id = "F"
aspect = 1.0
[[panel]]
id = "G"
aspect = 0.7
[[panel]]
id = "H"
aspect = 1.3
[[panel]]
id = "I"
aspect = 1.0
[[panel]]
id = "J"
aspect = 1.8
[[panel]]
id = "K"
aspect = 0.9

[[group]]
id = "root"
kind = "composite"
children = ["main", "extra"]

[[group]]
id = "main"
kind = "atomic"
priority = 1
panels = ["A", "B", "C", "D", "E"]

[[group]]
id = "extra"
kind = "atomic"
priority = 1
panels = ["F", "G", "H", "I", "J", "K"]
"#;
        let spec = FigureSpec::from_toml(toml).unwrap();
        let start = std::time::Instant::now();
        let layout = solve(&spec);
        let elapsed = start.elapsed();

        assert_eq!(layout.placements.len(), 11);
        assert!(layout.height_mm <= 216.0);
        assert!(
            elapsed.as_millis() < 500,
            "took {}ms, expected < 500ms",
            elapsed.as_millis()
        );

        eprintln!(
            "11-panel solve: {}ms, cost={:.4}",
            elapsed.as_millis(),
            layout.cost
        );
        for p in &layout.placements {
            eprintln!(
                "  {} ({}): {:.1}×{:.1}mm  aspect {:.2} (target {:.2})",
                p.panel_id, p.group_id, p.w_mm, p.h_mm, p.aspect_actual, p.aspect_target,
            );
        }
    }

    #[test]
    fn min_width_constraint_enforced() {
        let layout = solve_toml(
            r#"
[page]
width_mm = 160
max_height_mm = 216

[figure]
name = "minw"

[[panel]]
id = "A"
aspect = 1.0
min_width = 0.50

[[panel]]
id = "B"
aspect = 1.0

[[group]]
id = "root"
kind = "atomic"
panels = ["A", "B"]
"#,
        );
        assert_eq!(layout.placements.len(), 2);
        let a = layout
            .placements
            .iter()
            .find(|p| p.panel_id == "A")
            .unwrap();
        // A should get at least 50% of page width
        assert!(
            a.rect.w >= 0.49,
            "A width fraction {:.3} should be >= 0.50",
            a.rect.w
        );
    }

    #[test]
    fn min_height_constraint_enforced() {
        let layout = solve_toml(
            r#"
[page]
width_mm = 160
max_height_mm = 216

[figure]
name = "minh"

[[panel]]
id = "A"
aspect = 2.0
min_height = 0.40

[[panel]]
id = "B"
aspect = 1.0

[[group]]
id = "root"
kind = "atomic"
panels = ["A", "B"]
"#,
        );
        assert_eq!(layout.placements.len(), 2);
        let a = layout
            .placements
            .iter()
            .find(|p| p.panel_id == "A")
            .unwrap();
        // A should get at least 40% of page max height
        let a_h_frac = a.h_mm / layout.page_max_height_mm;
        assert!(
            a_h_frac >= 0.39,
            "A height fraction {a_h_frac:.3} should be >= 0.40"
        );
    }

    #[test]
    fn direction_column_forces_horizontal_split() {
        let layout = solve_toml(
            r#"
[page]
width_mm = 160
max_height_mm = 216

[figure]
name = "dir"

[[panel]]
id = "A"
aspect = 1.0

[[panel]]
id = "B"
aspect = 1.0

[[panel]]
id = "C"
aspect = 3.0

[[group]]
id = "root"
kind = "composite"
direction = "column"
children = ["top", "bottom"]

[[group]]
id = "top"
kind = "atomic"
panels = ["A", "B"]

[[group]]
id = "bottom"
kind = "atomic"
panels = ["C"]
"#,
        );
        assert_eq!(layout.placements.len(), 3);
        let c = layout
            .placements
            .iter()
            .find(|p| p.panel_id == "C")
            .unwrap();
        let a = layout
            .placements
            .iter()
            .find(|p| p.panel_id == "A")
            .unwrap();
        // C should be below A+B (column direction → horizontal split).
        // C's y position should be greater than A's.
        assert!(
            c.rect.y > a.rect.y + a.rect.h * 0.5,
            "C (y={:.3}) should be below A (y={:.3}, h={:.3})",
            c.rect.y,
            a.rect.y,
            a.rect.h
        );
        // C should span full width (not be squeezed to one side).
        assert!(
            c.rect.w > 0.9,
            "C width {:.3} should span (nearly) full width with direction=column",
            c.rect.w
        );
    }

    #[test]
    fn direction_row_forces_vertical_split() {
        let layout = solve_toml(
            r#"
[page]
width_mm = 160
max_height_mm = 216

[figure]
name = "dir_row"

[[panel]]
id = "A"
aspect = 0.5

[[panel]]
id = "B"
aspect = 0.5

[[group]]
id = "root"
kind = "composite"
direction = "row"
children = ["left", "right"]

[[group]]
id = "left"
kind = "atomic"
panels = ["A"]

[[group]]
id = "right"
kind = "atomic"
panels = ["B"]
"#,
        );
        assert_eq!(layout.placements.len(), 2);
        let a = layout
            .placements
            .iter()
            .find(|p| p.panel_id == "A")
            .unwrap();
        let b = layout
            .placements
            .iter()
            .find(|p| p.panel_id == "B")
            .unwrap();
        // B should be to the right of A (row direction → vertical split).
        assert!(
            b.rect.x > a.rect.x + a.rect.w * 0.5,
            "B (x={:.3}) should be right of A (x={:.3}, w={:.3})",
            b.rect.x,
            a.rect.x,
            a.rect.w
        );
    }

    #[test]
    fn proximity_scoring_works() {
        let layout = solve_toml(
            r#"
[page]
width_mm = 160
max_height_mm = 216

[figure]
name = "prox"

[[panel]]
id = "A"
aspect = 1.0
near = ["B"]

[[panel]]
id = "B"
aspect = 1.0
near = ["A"]

[[panel]]
id = "C"
aspect = 1.0

[[group]]
id = "root"
kind = "composite"
children = ["g1", "g2"]

[[group]]
id = "g1"
kind = "atomic"
panels = ["A", "B"]

[[group]]
id = "g2"
kind = "atomic"
panels = ["C"]
"#,
        );
        assert_eq!(layout.placements.len(), 3);
        // A and B should be adjacent (in the same group).
        let a = layout
            .placements
            .iter()
            .find(|p| p.panel_id == "A")
            .unwrap();
        let b = layout
            .placements
            .iter()
            .find(|p| p.panel_id == "B")
            .unwrap();
        // They should share an edge (same group, adjacent in sequence).
        let shared = shares_edge(&a.rect, &b.rect, 160.0, 216.0);
        assert!(shared, "A and B should share an edge");
    }

    #[test]
    fn inset_panel_placed_in_host() {
        let layout = solve_toml(
            r#"
[page]
width_mm = 160
max_height_mm = 216

[figure]
name = "inset_test"

[[panel]]
id = "A"
aspect = 1.356

  [[panel.inset]]
  anchor = "top-right"
  w_frac = 0.45
  h_frac = 0.40
  panel = "B"

[[panel]]
id = "B"
aspect = 1.2

[[panel]]
id = "C"
aspect = 1.5

[[group]]
id = "root"
kind = "composite"
children = ["arch", "bench"]

[[group]]
id = "arch"
kind = "atomic"
panels = ["A", "B"]

[[group]]
id = "bench"
kind = "atomic"
panels = ["C"]
"#,
        );
        // B should not be in main placements (it's an inset).
        assert!(
            !layout.placements.iter().any(|p| p.panel_id == "B"),
            "B should not be in main placements"
        );
        // A and C should be in main placements.
        assert_eq!(layout.placements.len(), 2);
        assert!(layout.placements.iter().any(|p| p.panel_id == "A"));
        assert!(layout.placements.iter().any(|p| p.panel_id == "C"));

        // B should be in inset placements.
        assert_eq!(layout.inset_placements.len(), 1);
        let b_inset = &layout.inset_placements[0];
        assert_eq!(b_inset.panel_id, "B");
        assert_eq!(b_inset.host_id, "A");

        // B's rect should be within A's rect.
        let a = layout
            .placements
            .iter()
            .find(|p| p.panel_id == "A")
            .unwrap();
        assert!(
            b_inset.rect.x >= a.rect.x - 0.001,
            "B x={:.4} should be >= A x={:.4}",
            b_inset.rect.x,
            a.rect.x
        );
        assert!(
            b_inset.rect.y >= a.rect.y - 0.001,
            "B y={:.4} should be >= A y={:.4}",
            b_inset.rect.y,
            a.rect.y
        );
        assert!(
            b_inset.rect.x + b_inset.rect.w <= a.rect.x + a.rect.w + 0.001,
            "B right edge should be within A"
        );
        assert!(
            b_inset.rect.y + b_inset.rect.h <= a.rect.y + a.rect.h + 0.001,
            "B bottom edge should be within A"
        );

        // B should be in the top-right quadrant of A.
        let a_mid_x = a.rect.x + a.rect.w / 2.0;
        assert!(
            b_inset.rect.x >= a_mid_x - 0.01,
            "B should be in the right half of A"
        );

        eprintln!(
            "Inset B: {:.1}×{:.1}mm at ({:.4}, {:.4})",
            b_inset.w_mm, b_inset.h_mm, b_inset.rect.x, b_inset.rect.y
        );
    }

    #[test]
    fn inset_preserves_aspect_ratio() {
        let layout = solve_toml(
            r#"
[page]
width_mm = 160
max_height_mm = 216

[figure]
name = "inset_aspect"

[[panel]]
id = "A"
aspect = 2.0

  [[panel.inset]]
  anchor = "bottom-left"
  w_frac = 0.30
  h_frac = 0.50
  panel = "B"

[[panel]]
id = "B"
aspect = 1.0

[[group]]
id = "root"
kind = "atomic"
panels = ["A", "B"]
"#,
        );
        assert_eq!(layout.inset_placements.len(), 1);
        let b = &layout.inset_placements[0];
        // Aspect should be close to target (1.0).
        assert!(
            (b.aspect_actual - b.aspect_target).abs() < 0.01,
            "Inset aspect {:.3} should match target {:.3}",
            b.aspect_actual,
            b.aspect_target
        );
    }

    #[test]
    fn optional_low_priority_group_can_be_dropped() {
        let layout = solve_toml(
            r#"
[page]
width_mm = 160
max_height_mm = 216

[figure]
name = "optional_drop"

[[panel]]
id = "A"
aspect = 1.0

[[panel]]
id = "B"
aspect = 100.0

[[group]]
id = "root"
kind = "composite"
children = ["main", "extra"]

[[group]]
id = "main"
kind = "atomic"
priority = 1
panels = ["A"]

[[group]]
id = "extra"
kind = "atomic"
priority = 2
panels = ["B"]
"#,
        );
        assert!(layout.placements.iter().any(|p| p.panel_id == "A"));
        assert!(layout.dropped_groups.iter().any(|g| g.group_id == "extra"));
        assert!(layout.checks.passed);
    }

    #[test]
    fn flexible_group_honors_max_panels() {
        let layout = solve_toml(
            r#"
[page]
width_mm = 160
max_height_mm = 216

[[panel]]
id = "A"
aspect = 1.0
[[panel]]
id = "B"
aspect = 1.0
[[panel]]
id = "C"
aspect = 1.0

[[group]]
id = "root"
kind = "flexible"
min_panels = 1
max_panels = 2
panels = ["A", "B", "C"]
"#,
        );
        assert_eq!(layout.placements.len(), 2);
    }

    #[test]
    fn hard_constraint_failure_marks_check_failed() {
        let layout = solve_toml(
            r#"
[page]
width_mm = 160
max_height_mm = 216

[[panel]]
id = "A"
aspect = 1.0
[[panel]]
id = "B"
aspect = 1.0

[[constraint]]
kind = "left_of"
ids = ["A", "B"]
hard = true

[[constraint]]
kind = "left_of"
ids = ["B", "A"]
hard = true

[[group]]
id = "root"
kind = "atomic"
panels = ["A", "B"]
"#,
        );
        assert!(!layout.checks.passed);
        assert!(layout.constraint_violations.iter().any(|v| v.hard));
    }

    #[test]
    fn soft_constraint_contributes_score() {
        let layout = solve_toml(
            r#"
[page]
width_mm = 160
max_height_mm = 216

[[panel]]
id = "A"
aspect = 1.0
[[panel]]
id = "B"
aspect = 1.0

[[constraint]]
kind = "left_of"
ids = ["A", "B"]
weight = 5.0

[[constraint]]
kind = "left_of"
ids = ["B", "A"]
weight = 5.0

[[group]]
id = "root"
kind = "atomic"
panels = ["A", "B"]
"#,
        );
        assert!(layout.score.constraint > 0.0);
        assert!(layout.constraint_violations.iter().any(|v| !v.hard));
    }

    #[test]
    fn mosaic_mode_is_reported() {
        let layout = solve_toml(
            r#"
[page]
width_mm = 160
max_height_mm = 216

[solver]
mode = "mosaic"

[[panel]]
id = "A"
aspect = 1.0
[[panel]]
id = "B"
aspect = 1.0
[[panel]]
id = "C"
aspect = 1.0

[[group]]
id = "root"
kind = "atomic"
panels = ["A", "B", "C"]
"#,
        );
        assert_eq!(layout.solver_mode, "Mosaic");
        assert_eq!(layout.placements.len(), 3);
    }
}
