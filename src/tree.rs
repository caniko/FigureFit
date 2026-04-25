//! Guillotine tree data structure for recursive binary-split layouts.

use crate::config::Anchor;
use serde::Serialize;

/// Which sides of a panel have dead space due to aspect-ratio mismatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum VoidSide {
    /// No meaningful dead space.
    None,
    /// Dead space on left+right (content narrower than box).
    Horizontal,
    /// Dead space on top+bottom (content shorter than box).
    Vertical,
}

impl std::fmt::Display for VoidSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VoidSide::None => write!(f, "none"),
            VoidSide::Horizontal => write!(f, "horizontal"),
            VoidSide::Vertical => write!(f, "vertical"),
        }
    }
}

/// Compute the void fraction and direction from allocated vs target aspect.
///
/// Returns `(void_frac, void_side)` where `void_frac` is in `[0, 1)`.
pub fn compute_void(aspect_actual: f64, aspect_target: f64) -> (f64, VoidSide) {
    let (lo, hi) = if aspect_actual < aspect_target {
        (aspect_actual, aspect_target)
    } else {
        (aspect_target, aspect_actual)
    };
    let void_frac = 1.0 - lo / hi;
    if void_frac < 1e-6 {
        (0.0, VoidSide::None)
    } else if aspect_target < aspect_actual {
        // Content is taller than box → height-constrained, fills height,
        // fitted_w = H * target < W → horizontal dead space.
        (void_frac, VoidSide::Horizontal)
    } else {
        // Content is wider than box → width-constrained, fills width,
        // fitted_h = W / target < H → vertical dead space.
        (void_frac, VoidSide::Vertical)
    }
}

/// Direction of a guillotine cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SplitDir {
    /// Cut horizontally: top child and bottom child.
    Horizontal,
    /// Cut vertically: left child and right child.
    Vertical,
}

/// Discretized split ratios — standard scientific proportions.
///
/// Each ratio represents how much of the dimension goes to the first child.
pub const SPLIT_RATIOS: &[f64] = &[
    0.167, // 1:5
    0.200, // 1:4
    0.250, // 1:3
    0.333, // 1:2
    0.400, // 2:3
    0.500, // 1:1
    0.600, // 3:2
    0.667, // 2:1
    0.750, // 3:1
    0.800, // 4:1
    0.833, // 5:1
];

/// A node in the guillotine tree.
#[derive(Debug, Clone)]
pub enum GNode {
    /// A leaf node containing a single panel.
    Leaf {
        /// Index into the flattened panel sequence.
        panel_idx: usize,
        /// Which alternative was chosen (None = primary aspect).
        alt_chosen: Option<String>,
    },
    /// An internal node representing a binary split.
    Split {
        dir: SplitDir,
        /// Fraction of the dimension allocated to the first (left/top) child.
        ratio: f64,
        first: Box<GNode>,
        second: Box<GNode>,
    },
}

/// A rectangle in fractional coordinates [0, 1].
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    /// Full unit rectangle.
    pub fn unit() -> Self {
        Self::new(0.0, 0.0, 1.0, 1.0)
    }

    /// Aspect ratio (width / height) given physical page dimensions.
    pub fn aspect(&self, page_w: f64, page_h: f64) -> f64 {
        (self.w * page_w) / (self.h * page_h)
    }

    /// Split vertically: left child gets `ratio` of width.
    pub fn split_v(self, ratio: f64) -> (Rect, Rect) {
        let left = Rect::new(self.x, self.y, self.w * ratio, self.h);
        let right = Rect::new(
            self.x + self.w * ratio,
            self.y,
            self.w * (1.0 - ratio),
            self.h,
        );
        (left, right)
    }

    /// Split horizontally: top child gets `ratio` of height.
    pub fn split_h(self, ratio: f64) -> (Rect, Rect) {
        let top = Rect::new(self.x, self.y, self.w, self.h * ratio);
        let bottom = Rect::new(
            self.x,
            self.y + self.h * ratio,
            self.w,
            self.h * (1.0 - ratio),
        );
        (top, bottom)
    }

    /// Axis-aligned bounding box union of two rects.
    pub fn union(a: Rect, b: Rect) -> Rect {
        let x = a.x.min(b.x);
        let y = a.y.min(b.y);
        let x2 = (a.x + a.w).max(b.x + b.w);
        let y2 = (a.y + a.h).max(b.y + b.h);
        Rect::new(x, y, x2 - x, y2 - y)
    }
}

/// Resolved placement for a single panel.
#[derive(Debug, Clone, Serialize)]
pub struct Placement {
    pub panel_id: String,
    pub group_id: String,
    pub rect: Rect,
    pub w_mm: f64,
    pub h_mm: f64,
    pub aspect_actual: f64,
    pub aspect_target: f64,
    pub alt_chosen: Option<String>,
    /// Content padding in mm (pass-through from TOML spec).
    pub pad_mm: f64,
    pub label_gutter_mm: Option<f64>,
    pub legend_margin_mm: Option<f64>,
    /// Fraction of panel area that is void (dead space), in [0, 1).
    pub void_frac: f64,
    /// Which sides have dead space.
    pub void_side: VoidSide,
}

/// A panel placed inside another panel's dead space.
#[derive(Debug, Clone, Serialize)]
pub struct InsetPlacement {
    /// The inset (tenant) panel ID.
    pub panel_id: String,
    /// The host panel ID.
    pub host_id: String,
    /// Group the inset panel belongs to.
    pub group_id: String,
    /// Corner anchor within the host panel.
    pub anchor: Anchor,
    /// Placement rect in figure-fraction coordinates.
    pub rect: Rect,
    pub w_mm: f64,
    pub h_mm: f64,
    pub aspect_actual: f64,
    pub aspect_target: f64,
    pub alt_chosen: Option<String>,
    /// Content padding in mm (pass-through from TOML spec).
    pub pad_mm: f64,
    pub label_gutter_mm: Option<f64>,
    pub legend_margin_mm: Option<f64>,
    /// Fraction of panel area that is void (dead space), in [0, 1).
    pub void_frac: f64,
    /// Which sides have dead space.
    pub void_side: VoidSide,
}

/// A fully solved layout.
#[derive(Debug, Clone, Serialize)]
pub struct SolvedLayout {
    pub figure_name: String,
    pub cost: f64,
    pub solver_mode: String,
    pub height_mm: f64,
    pub page_width_mm: f64,
    pub page_max_height_mm: f64,
    pub placements: Vec<Placement>,
    pub inset_placements: Vec<InsetPlacement>,
    pub dropped_groups: Vec<DroppedGroup>,
    pub score: ScoreComponents,
    pub constraint_violations: Vec<ConstraintViolation>,
    pub candidate_summaries: Vec<CandidateSummary>,
    pub checks: CheckSummary,
}

/// A group that was dropped because it didn't fit.
#[derive(Debug, Clone, Serialize)]
pub struct DroppedGroup {
    pub group_id: String,
    pub priority: u32,
    pub panel_ids: Vec<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ScoreComponents {
    pub aspect: f64,
    pub void: f64,
    pub dimension: f64,
    pub constraint: f64,
    pub optional_content: f64,
    pub required_content: f64,
    pub height_utilization: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConstraintViolation {
    pub kind: String,
    pub ids: Vec<String>,
    pub hard: bool,
    pub weight: f64,
    pub penalty: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CandidateSummary {
    pub rank: usize,
    pub cost: f64,
    pub height_mm: f64,
    pub panels: usize,
    pub dropped_groups: Vec<String>,
    pub rejected_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckSummary {
    pub passed: bool,
    pub issues: Vec<String>,
}

impl Default for CheckSummary {
    fn default() -> Self {
        Self {
            passed: true,
            issues: Vec::new(),
        }
    }
}

impl GNode {
    /// Flatten the tree into panel placements given a bounding rect.
    pub fn resolve(&self, bbox: Rect, _page_w: f64, _page_h: f64) -> Vec<(usize, Rect)> {
        match self {
            GNode::Leaf { panel_idx, .. } => vec![(*panel_idx, bbox)],
            GNode::Split {
                dir,
                ratio,
                first,
                second,
            } => {
                let (r1, r2) = match dir {
                    SplitDir::Vertical => bbox.split_v(*ratio),
                    SplitDir::Horizontal => bbox.split_h(*ratio),
                };
                let mut out = first.resolve(r1, _page_w, _page_h);
                out.extend(second.resolve(r2, _page_w, _page_h));
                out
            }
        }
    }
}
