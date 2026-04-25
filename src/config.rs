//! TOML configuration types for figure layout specifications.
//!
//! Panels are defined flat at the top level. Groups form a hierarchy
//! (tree) that controls how panels are organized and prioritized.
//! The `near` field uses a **unified namespace** — a reference can
//! point to either a panel ID or a group ID.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

// ── Top-level spec ──────────────────────────────────────────────────

/// Top-level figure specification parsed from TOML.
#[derive(Debug, Deserialize)]
pub struct FigureSpec {
    pub page: PageConfig,
    #[serde(default)]
    pub figure: FigureMeta,
    #[serde(default)]
    pub solver: SolverConfig,
    #[serde(default)]
    pub quality: QualityConfig,
    #[serde(rename = "panel")]
    pub panels: Vec<PanelDef>,
    #[serde(rename = "group")]
    pub groups: Vec<GroupDef>,
    #[serde(default, rename = "constraint")]
    pub constraints: Vec<ConstraintDef>,
}

// ── Page geometry ───────────────────────────────────────────────────

/// Physical page constraints and spacing.
#[derive(Debug, Deserialize)]
pub struct PageConfig {
    /// Available width in mm (linewidth after margins).
    pub width_mm: f64,
    /// Maximum available height in mm (textheight minus caption reserve).
    pub max_height_mm: f64,
    /// Gap between panels within a group (mm).
    #[serde(default = "default_panel_gap")]
    pub panel_gap_mm: f64,
    /// Gap between groups (mm) — larger than panel gap for visual separation.
    #[serde(default = "default_group_gap")]
    pub group_gap_mm: f64,
    /// Height reserved for panel labels (mm) at the top of each panel.
    /// Set to 0.0 to disable label gutters.
    #[serde(default)]
    pub label_height_mm: f64,
}

fn default_panel_gap() -> f64 {
    2.0
}
fn default_group_gap() -> f64 {
    4.0
}

// ── Figure metadata ─────────────────────────────────────────────────

#[derive(Debug, Default, Deserialize)]
pub struct FigureMeta {
    /// Figure identifier (e.g. "CF06"). Optional — derived from filename if omitted.
    #[serde(default)]
    pub name: Option<String>,
}

// ── Panel definitions ───────────────────────────────────────────────

/// A panel (leaf item) that can be placed in the figure.
#[derive(Debug, Clone, Deserialize)]
pub struct PanelDef {
    /// Unique identifier (e.g. "A", "B"). Must not collide with group IDs.
    pub id: String,
    /// Target aspect ratio (width / height).
    pub aspect: Option<f64>,
    /// Optional source asset used to derive aspect when `aspect` is omitted.
    pub source: Option<String>,
    /// Bounding box policy for source-derived aspect.
    #[serde(default)]
    pub bbox: BBoxMode,
    /// Override whether this panel is required.
    pub required: Option<bool>,
    /// Minimum width as fraction of page width.
    pub min_width: Option<f64>,
    /// Maximum width as fraction of page width.
    pub max_width: Option<f64>,
    /// Minimum height as fraction of page max height.
    /// Use for panels with below-axis legends, long rotated tick labels, etc.
    pub min_height: Option<f64>,
    /// IDs of panels or groups this should share an edge with.
    #[serde(default)]
    pub near: Vec<String>,
    /// Alternative aspect ratios to try (solver picks the best).
    #[serde(default, rename = "alt")]
    pub alts: Vec<AltDef>,
    /// Inset panels placed in this panel's dead space.
    #[serde(default, rename = "inset")]
    pub insets: Vec<InsetDef>,
    /// Content padding in mm, subtracted from figsize on each side by the
    /// export layer.  Default 0.0.  Set > 0 for tables / text-heavy panels.
    #[serde(default)]
    pub pad_mm: f64,
    /// Panel-specific label gutter, passed through to diagnostics/reporting.
    pub label_gutter_mm: Option<f64>,
    /// Extra margin for legends, passed through to diagnostics/reporting.
    pub legend_margin_mm: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BBoxMode {
    #[default]
    Media,
    Crop,
    Tight,
}

/// An alternative aspect ratio for a panel.
#[derive(Debug, Clone, Deserialize)]
pub struct AltDef {
    pub label: String,
    pub aspect: f64,
}

/// Corner anchor for an inset panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Anchor {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// An inset: a panel placed inside another panel's dead space.
#[derive(Debug, Clone, Deserialize)]
pub struct InsetDef {
    /// Corner of the host panel where the inset is placed.
    pub anchor: Anchor,
    /// Fraction of the host panel's width available for the inset.
    pub w_frac: f64,
    /// Fraction of the host panel's height available for the inset.
    pub h_frac: f64,
    /// ID of the panel to place in this inset slot.
    pub panel: String,
    /// Optional gap between inset and host content (mm).
    /// Defaults to the page's `panel_gap_mm`.
    pub gap_mm: Option<f64>,
}

impl PanelDef {
    /// All candidate aspect ratios (primary + alternatives).
    pub fn candidate_aspects(&self) -> Vec<(Option<&str>, f64)> {
        let mut v = self
            .aspect
            .map_or_else(Vec::new, |aspect| vec![(None, aspect)]);
        for alt in &self.alts {
            v.push((Some(alt.label.as_str()), alt.aspect));
        }
        v
    }

    pub fn required(&self, group_priority: u32) -> bool {
        self.required.unwrap_or(group_priority == 1)
    }
}

// ── Solver and quality options ──────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SolverMode {
    #[default]
    Guillotine,
    Mosaic,
    Auto,
}

#[derive(Debug, Default, Deserialize)]
pub struct SolverConfig {
    #[serde(default)]
    pub mode: SolverMode,
}

#[derive(Debug, Default, Deserialize)]
pub struct QualityConfig {
    pub min_height_utilization: Option<f64>,
    pub max_void_frac: Option<f64>,
    pub max_soft_violations: Option<usize>,
}

// ── Normalized constraints ──────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintKind {
    Near,
    SameRow,
    SameColumn,
    LeftOf,
    RightOf,
    Above,
    Below,
    AlignTop,
    AlignBottom,
    AlignLeft,
    AlignRight,
    EqualWidth,
    EqualHeight,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConstraintDef {
    pub kind: ConstraintKind,
    #[serde(default)]
    pub ids: Vec<String>,
    #[serde(default)]
    pub hard: bool,
    #[serde(default = "default_constraint_weight")]
    pub weight: f64,
}

fn default_constraint_weight() -> f64 {
    1.0
}

// ── Group definitions ───────────────────────────────────────────────

/// How a group behaves during layout resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupKind {
    /// All panels must be included, or the entire group is dropped.
    Atomic,
    /// At least `min_panels` must be included; the rest are optional.
    /// The solver picks the subset that produces the best layout.
    Flexible,
    /// Exactly one child group is selected (mutually exclusive children).
    OneOf,
    /// Contains child groups arranged by priority. All children are
    /// included if possible; lower-priority children may be dropped.
    Composite,
}

/// Layout direction for composite groups.
///
/// Constrains how the solver splits between children of a composite group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Children arranged in a row (side-by-side). Solver uses vertical cuts.
    Row,
    /// Children arranged in a column (stacked top-to-bottom). Solver uses horizontal cuts.
    Column,
}

/// A group in the layout hierarchy.
///
/// Groups form a tree. Leaf groups (`atomic` / `flexible`) contain panel
/// IDs. Branch groups (`composite` / `one_of`) contain child group IDs.
#[derive(Debug, Deserialize)]
pub struct GroupDef {
    /// Unique identifier. Must not collide with panel IDs.
    pub id: String,
    pub kind: GroupKind,
    /// Lower value = higher importance (1 = most important).
    #[serde(default = "default_priority")]
    pub priority: u32,
    /// Override whether this group is required. Defaults to priority == 1.
    pub required: Option<bool>,
    /// Panel IDs directly in this group (leaf groups only).
    #[serde(default)]
    pub panels: Vec<String>,
    /// Child group IDs (branch groups only).
    #[serde(default)]
    pub children: Vec<String>,
    /// IDs of panels or groups that should be adjacent.
    /// Uses the unified namespace (can reference panels or groups).
    #[serde(default)]
    pub near: Vec<String>,
    /// Minimum panels required (only for `flexible` kind).
    pub min_panels: Option<usize>,
    /// Maximum panels included (only for `flexible` kind).
    pub max_panels: Option<usize>,
    /// Layout direction for composite groups.
    /// Constrains the split direction at boundaries between children.
    /// Only meaningful for `composite` groups; ignored for leaf/one_of groups.
    pub direction: Option<Direction>,
}

impl GroupDef {
    pub fn required(&self) -> bool {
        self.required.unwrap_or(self.priority == 1)
    }
}

fn default_priority() -> u32 {
    1
}

// ── Parsing ─────────────────────────────────────────────────────────

impl FigureSpec {
    /// Parse a TOML string into a FigureSpec.
    pub fn from_toml(s: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(s)
    }

    /// Build a lookup map from panel ID → PanelDef.
    pub fn panel_map(&self) -> HashMap<&str, &PanelDef> {
        self.panels.iter().map(|p| (p.id.as_str(), p)).collect()
    }

    /// Build a lookup map from group ID → GroupDef.
    pub fn group_map(&self) -> HashMap<&str, &GroupDef> {
        self.groups.iter().map(|g| (g.id.as_str(), g)).collect()
    }

    /// Derive missing panel aspects from SVG source metadata.
    ///
    /// Only `bbox = "media"` is currently supported. PDF aspect derivation and
    /// crop/tight bbox policies are intentionally deferred so specs do not
    /// silently get a different interpretation than requested.
    pub fn derive_content_aspects(&mut self, base_dir: &Path) -> Result<(), String> {
        for panel in &mut self.panels {
            if panel.aspect.is_some() {
                continue;
            }
            let Some(source) = &panel.source else {
                continue;
            };
            if panel.bbox != BBoxMode::Media {
                return Err(format!(
                    "Panel '{}': bbox policy {:?} is not yet supported for source-derived aspects; \
                     use bbox = \"media\" or specify aspect explicitly",
                    panel.id, panel.bbox
                ));
            }
            let source_path = resolve_source_path(base_dir, source);
            panel.aspect = Some(derive_aspect_from_source(&source_path).map_err(|e| {
                format!(
                    "Panel '{}': failed to derive aspect from {}: {e}",
                    panel.id,
                    source_path.display()
                )
            })?);
        }
        Ok(())
    }

    /// Expand legacy `near` fields and explicit constraints into one list.
    pub fn normalized_constraints(&self) -> Vec<ConstraintDef> {
        let mut constraints = self.constraints.clone();
        for panel in &self.panels {
            for target in &panel.near {
                constraints.push(ConstraintDef {
                    kind: ConstraintKind::Near,
                    ids: vec![panel.id.clone(), target.clone()],
                    hard: false,
                    weight: 1.0,
                });
            }
        }
        for group in &self.groups {
            for target in &group.near {
                constraints.push(ConstraintDef {
                    kind: ConstraintKind::Near,
                    ids: vec![group.id.clone(), target.clone()],
                    hard: false,
                    weight: 1.0,
                });
            }
        }
        constraints
    }
}

// ── Preflight validation ────────────────────────────────────────────

/// A preflight validation error.
#[derive(Debug, Clone)]
pub struct PreflightError {
    pub message: String,
}

impl std::fmt::Display for PreflightError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl FigureSpec {
    /// Run all preflight checks. Returns a list of errors (empty = OK).
    pub fn preflight(&self) -> Vec<PreflightError> {
        let mut errors = Vec::new();
        let panel_ids: HashSet<&str> = self.panels.iter().map(|p| p.id.as_str()).collect();
        let group_ids: HashSet<&str> = self.groups.iter().map(|g| g.id.as_str()).collect();
        let group_map = self.group_map();

        for p in &self.panels {
            if p.aspect.is_none() && p.source.is_none() {
                errors.push(PreflightError {
                    message: format!("Panel '{}': must specify aspect or source", p.id),
                });
            }
            if let Some(aspect) = p.aspect
                && aspect <= 0.0
            {
                errors.push(PreflightError {
                    message: format!("Panel '{}': aspect must be > 0", p.id),
                });
            }
        }

        // 1. No ID collisions between panels and groups.
        for pid in &panel_ids {
            if group_ids.contains(pid) {
                errors.push(PreflightError {
                    message: format!("ID collision: '{pid}' is both a panel and a group"),
                });
            }
        }

        // 2. No duplicate panel IDs.
        let mut seen = HashSet::new();
        for p in &self.panels {
            if !seen.insert(p.id.as_str()) {
                errors.push(PreflightError {
                    message: format!("Duplicate panel ID: '{}'", p.id),
                });
            }
        }

        // 3. No duplicate group IDs.
        seen.clear();
        for g in &self.groups {
            if !seen.insert(g.id.as_str()) {
                errors.push(PreflightError {
                    message: format!("Duplicate group ID: '{}'", g.id),
                });
            }
        }

        // 4. Group structural validation.
        for g in &self.groups {
            match g.kind {
                GroupKind::Atomic | GroupKind::Flexible => {
                    if g.panels.is_empty() {
                        errors.push(PreflightError {
                            message: format!(
                                "Group '{}' ({:?}): must have at least one panel",
                                g.id, g.kind
                            ),
                        });
                    }
                    if !g.children.is_empty() {
                        errors.push(PreflightError {
                            message: format!(
                                "Group '{}' ({:?}): leaf groups cannot have children",
                                g.id, g.kind
                            ),
                        });
                    }
                    for pid in &g.panels {
                        if !panel_ids.contains(pid.as_str()) {
                            errors.push(PreflightError {
                                message: format!(
                                    "Group '{}': references unknown panel '{pid}'",
                                    g.id
                                ),
                            });
                        }
                    }
                    if g.kind == GroupKind::Flexible {
                        let min = g.min_panels.unwrap_or(0);
                        if min == 0 || min > g.panels.len() {
                            errors.push(PreflightError {
                                message: format!(
                                    "Group '{}' (flexible): min_panels must be in 1..={}",
                                    g.id,
                                    g.panels.len()
                                ),
                            });
                        }
                        if let Some(max) = g.max_panels
                            && (max < min || max > g.panels.len())
                        {
                            errors.push(PreflightError {
                                message: format!(
                                    "Group '{}' (flexible): max_panels must be in {}..={}",
                                    g.id,
                                    min,
                                    g.panels.len()
                                ),
                            });
                        }
                    }
                }
                GroupKind::Composite | GroupKind::OneOf => {
                    if g.children.is_empty() {
                        errors.push(PreflightError {
                            message: format!(
                                "Group '{}' ({:?}): must have at least one child",
                                g.id, g.kind
                            ),
                        });
                    }
                    if !g.panels.is_empty() {
                        errors.push(PreflightError {
                            message: format!(
                                "Group '{}' ({:?}): branch groups cannot have panels directly",
                                g.id, g.kind
                            ),
                        });
                    }
                    for cid in &g.children {
                        if !group_ids.contains(cid.as_str()) {
                            errors.push(PreflightError {
                                message: format!(
                                    "Group '{}': references unknown child group '{cid}'",
                                    g.id
                                ),
                            });
                        }
                    }
                }
            }
        }

        // 5. `near` references must exist in the unified namespace.
        let all_ids: HashSet<&str> = panel_ids.union(&group_ids).copied().collect();
        for p in &self.panels {
            for nid in &p.near {
                if !all_ids.contains(nid.as_str()) {
                    errors.push(PreflightError {
                        message: format!("Panel '{}': near reference '{nid}' not found", p.id),
                    });
                }
            }
        }
        for g in &self.groups {
            for nid in &g.near {
                if !all_ids.contains(nid.as_str()) {
                    errors.push(PreflightError {
                        message: format!("Group '{}': near reference '{nid}' not found", g.id),
                    });
                }
            }
        }
        for c in &self.constraints {
            if c.ids.len() < 2 {
                errors.push(PreflightError {
                    message: format!(
                        "Constraint {:?}: ids must contain at least 2 entries",
                        c.kind
                    ),
                });
            }
            if c.weight < 0.0 {
                errors.push(PreflightError {
                    message: format!("Constraint {:?}: weight must be >= 0", c.kind),
                });
            }
            for id in &c.ids {
                if !all_ids.contains(id.as_str()) {
                    errors.push(PreflightError {
                        message: format!("Constraint {:?}: reference '{id}' not found", c.kind),
                    });
                }
            }
        }

        // 6. Inset validation.
        let mut inset_panels: HashSet<&str> = HashSet::new();
        for p in &self.panels {
            for inset in &p.insets {
                // 6a. Inset panel must exist.
                if !panel_ids.contains(inset.panel.as_str()) {
                    errors.push(PreflightError {
                        message: format!(
                            "Panel '{}': inset references unknown panel '{}'",
                            p.id, inset.panel
                        ),
                    });
                }
                // 6b. Inset panel must not be the host itself.
                if inset.panel == p.id {
                    errors.push(PreflightError {
                        message: format!("Panel '{}': inset panel cannot be the host itself", p.id),
                    });
                }
                // 6c. w_frac and h_frac must be in (0, 1].
                if inset.w_frac <= 0.0 || inset.w_frac > 1.0 {
                    errors.push(PreflightError {
                        message: format!(
                            "Panel '{}': inset w_frac must be in (0, 1], got {}",
                            p.id, inset.w_frac
                        ),
                    });
                }
                if inset.h_frac <= 0.0 || inset.h_frac > 1.0 {
                    errors.push(PreflightError {
                        message: format!(
                            "Panel '{}': inset h_frac must be in (0, 1], got {}",
                            p.id, inset.h_frac
                        ),
                    });
                }
                // 6d. A panel can only be an inset once.
                if !inset_panels.insert(inset.panel.as_str()) {
                    errors.push(PreflightError {
                        message: format!(
                            "Panel '{}' is used as an inset more than once",
                            inset.panel
                        ),
                    });
                }
            }
        }

        // 7. Shared panels in atomic groups: a panel can only be in ONE atomic group.
        let mut atomic_owners: HashMap<&str, &str> = HashMap::new();
        for g in &self.groups {
            if g.kind == GroupKind::Atomic {
                for pid in &g.panels {
                    if let Some(prev) = atomic_owners.insert(pid.as_str(), g.id.as_str()) {
                        errors.push(PreflightError {
                            message: format!(
                                "Panel '{pid}' is in two atomic groups: '{prev}' and '{}'. \
                                 Shared panels must use flexible groups.",
                                g.id
                            ),
                        });
                    }
                }
            }
        }

        // 8. No cycles in group hierarchy.
        if has_cycle(&group_map) {
            errors.push(PreflightError {
                message: "Cycle detected in group hierarchy".into(),
            });
        }

        // 9. Exactly one root group (not referenced as a child by anyone).
        let children_of_any: HashSet<&str> = self
            .groups
            .iter()
            .flat_map(|g| g.children.iter().map(String::as_str))
            .collect();
        let roots: Vec<&str> = group_ids
            .iter()
            .filter(|id| !children_of_any.contains(**id))
            .copied()
            .collect();
        if roots.is_empty() {
            errors.push(PreflightError {
                message: "No root group found (all groups are children of another)".into(),
            });
        } else if roots.len() > 1 {
            errors.push(PreflightError {
                message: format!(
                    "Multiple root groups found: {roots:?}. Wrap them in a composite root."
                ),
            });
        }

        errors
    }

    /// Collect panel IDs that are insets (placed inside another panel).
    pub fn inset_panel_ids(&self) -> HashSet<String> {
        self.panels
            .iter()
            .flat_map(|p| p.insets.iter().map(|i| i.panel.clone()))
            .collect()
    }

    /// Find the root group ID (the one not referenced as a child).
    pub fn root_group_id(&self) -> Option<&str> {
        let children_of_any: HashSet<&str> = self
            .groups
            .iter()
            .flat_map(|g| g.children.iter().map(String::as_str))
            .collect();
        self.groups
            .iter()
            .map(|g| g.id.as_str())
            .find(|id| !children_of_any.contains(id))
    }
}

fn resolve_source_path(base_dir: &Path, source: &str) -> PathBuf {
    let path = PathBuf::from(source);
    if path.is_absolute() {
        path
    } else {
        base_dir.join(path)
    }
}

fn derive_aspect_from_source(path: &Path) -> Result<f64, String> {
    match path.extension().and_then(|s| s.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("svg") => derive_svg_aspect(path),
        Some(ext) if ext.eq_ignore_ascii_case("pdf") => {
            Err("PDF aspect derivation is not yet available; specify aspect explicitly".into())
        }
        Some(ext) => Err(format!("unsupported source extension .{ext}")),
        None => Err("source has no file extension".into()),
    }
}

fn derive_svg_aspect(path: &Path) -> Result<f64, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    if let Some(view_box) = attr_value(&text, "viewBox").or_else(|| attr_value(&text, "viewbox")) {
        let nums: Vec<f64> = view_box
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse().ok())
            .collect();
        if nums.len() == 4 && nums[2] > 0.0 && nums[3] > 0.0 {
            return Ok(nums[2] / nums[3]);
        }
    }
    let width = attr_value(&text, "width").and_then(|s| parse_svg_length(&s));
    let height = attr_value(&text, "height").and_then(|s| parse_svg_length(&s));
    match (width, height) {
        (Some(w), Some(h)) if w > 0.0 && h > 0.0 => Ok(w / h),
        _ => Err("SVG must have viewBox or width/height metadata".into()),
    }
}

fn attr_value(text: &str, attr: &str) -> Option<String> {
    let needle = format!("{attr}=");
    let start = text.find(&needle)? + needle.len();
    let quote = text[start..].chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &text[start + quote.len_utf8()..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

fn parse_svg_length(raw: &str) -> Option<f64> {
    let number: String = raw
        .chars()
        .take_while(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+'))
        .collect();
    number.parse().ok()
}

/// Detect cycles in the group hierarchy using DFS.
fn has_cycle(groups: &HashMap<&str, &GroupDef>) -> bool {
    let mut visited = HashSet::new();
    let mut stack = HashSet::new();

    fn dfs<'a>(
        id: &'a str,
        groups: &HashMap<&'a str, &'a GroupDef>,
        visited: &mut HashSet<&'a str>,
        stack: &mut HashSet<&'a str>,
    ) -> bool {
        if stack.contains(id) {
            return true;
        }
        if visited.contains(id) {
            return false;
        }
        visited.insert(id);
        stack.insert(id);
        if let Some(g) = groups.get(id) {
            for child in &g.children {
                if dfs(child.as_str(), groups, visited, stack) {
                    return true;
                }
            }
        }
        stack.remove(id);
        false
    }

    for id in groups.keys() {
        if dfs(id, groups, &mut visited, &mut stack) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hierarchical() {
        let toml = r#"
[page]
width_mm = 160
max_height_mm = 216
panel_gap_mm = 2.0
group_gap_mm = 4.0
label_height_mm = 3.5

[figure]
name = "CF04"

[[panel]]
id = "A"
aspect = 0.75

[[panel]]
id = "B"
aspect = 1.5
near = ["A"]

  [[panel.alt]]
  label = "wide"
  aspect = 2.0

[[panel]]
id = "C"
aspect = 1.2

[[panel]]
id = "D"
aspect = 1.0

[[group]]
id = "root"
kind = "composite"
children = ["arch", "bench"]

[[group]]
id = "arch"
kind = "atomic"
priority = 1
panels = ["A"]

[[group]]
id = "bench"
kind = "composite"
priority = 1
children = ["queries", "other"]
near = ["arch"]

[[group]]
id = "queries"
kind = "atomic"
priority = 1
panels = ["B", "C"]

[[group]]
id = "other"
kind = "atomic"
priority = 2
panels = ["D"]
"#;
        let spec = FigureSpec::from_toml(toml).unwrap();
        assert_eq!(spec.panels.len(), 4);
        assert_eq!(spec.groups.len(), 5);
        assert_eq!(spec.root_group_id(), Some("root"));

        let errors = spec.preflight();
        assert!(errors.is_empty(), "errors: {errors:?}");
    }

    #[test]
    fn preflight_catches_id_collision() {
        let toml = r#"
[page]
width_mm = 160
max_height_mm = 216

[figure]
name = "test"

[[panel]]
id = "X"
aspect = 1.0

[[group]]
id = "X"
kind = "atomic"
panels = ["X"]
"#;
        let spec = FigureSpec::from_toml(toml).unwrap();
        let errors = spec.preflight();
        assert!(errors.iter().any(|e| e.message.contains("ID collision")));
    }

    #[test]
    fn preflight_catches_cycle() {
        let toml = r#"
[page]
width_mm = 160
max_height_mm = 216

[figure]
name = "test"

[[panel]]
id = "A"
aspect = 1.0

[[group]]
id = "g1"
kind = "composite"
children = ["g2"]

[[group]]
id = "g2"
kind = "composite"
children = ["g1"]
"#;
        let spec = FigureSpec::from_toml(toml).unwrap();
        let errors = spec.preflight();
        assert!(errors.iter().any(|e| e.message.contains("Cycle")));
    }

    #[test]
    fn preflight_catches_shared_panel_in_two_atomics() {
        let toml = r#"
[page]
width_mm = 160
max_height_mm = 216

[figure]
name = "test"

[[panel]]
id = "E"
aspect = 1.5

[[group]]
id = "root"
kind = "composite"
children = ["g1", "g2"]

[[group]]
id = "g1"
kind = "atomic"
panels = ["E"]

[[group]]
id = "g2"
kind = "atomic"
panels = ["E"]
"#;
        let spec = FigureSpec::from_toml(toml).unwrap();
        let errors = spec.preflight();
        assert!(errors.iter().any(|e| e.message.contains("two atomic")));
    }

    #[test]
    fn preflight_valid_inset() {
        let toml = r#"
[page]
width_mm = 160
max_height_mm = 216

[figure]
name = "test"

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

[[group]]
id = "root"
kind = "atomic"
panels = ["A", "B"]
"#;
        let spec = FigureSpec::from_toml(toml).unwrap();
        let errors = spec.preflight();
        assert!(errors.is_empty(), "errors: {errors:?}");
    }

    #[test]
    fn preflight_catches_unknown_inset_panel() {
        let toml = r#"
[page]
width_mm = 160
max_height_mm = 216

[figure]
name = "test"

[[panel]]
id = "A"
aspect = 1.0

  [[panel.inset]]
  anchor = "top-left"
  w_frac = 0.5
  h_frac = 0.5
  panel = "Z"

[[group]]
id = "root"
kind = "atomic"
panels = ["A"]
"#;
        let spec = FigureSpec::from_toml(toml).unwrap();
        let errors = spec.preflight();
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("unknown panel 'Z'"))
        );
    }

    #[test]
    fn preflight_catches_self_inset() {
        let toml = r#"
[page]
width_mm = 160
max_height_mm = 216

[figure]
name = "test"

[[panel]]
id = "A"
aspect = 1.0

  [[panel.inset]]
  anchor = "top-right"
  w_frac = 0.5
  h_frac = 0.5
  panel = "A"

[[group]]
id = "root"
kind = "atomic"
panels = ["A"]
"#;
        let spec = FigureSpec::from_toml(toml).unwrap();
        let errors = spec.preflight();
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("cannot be the host itself"))
        );
    }

    #[test]
    fn preflight_catches_invalid_inset_frac() {
        let toml = r#"
[page]
width_mm = 160
max_height_mm = 216

[figure]
name = "test"

[[panel]]
id = "A"
aspect = 1.0

  [[panel.inset]]
  anchor = "bottom-right"
  w_frac = 1.5
  h_frac = 0.5
  panel = "B"

[[panel]]
id = "B"
aspect = 1.0

[[group]]
id = "root"
kind = "atomic"
panels = ["A", "B"]
"#;
        let spec = FigureSpec::from_toml(toml).unwrap();
        let errors = spec.preflight();
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("w_frac must be in (0, 1]"))
        );
    }

    #[test]
    fn derive_aspect_from_svg_source() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("panel.svg"),
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 100"></svg>"#,
        )
        .unwrap();
        let mut spec = FigureSpec::from_toml(
            r#"
[page]
width_mm = 160
max_height_mm = 216

[[panel]]
id = "A"
source = "panel.svg"

[[group]]
id = "root"
kind = "atomic"
panels = ["A"]
"#,
        )
        .unwrap();
        spec.derive_content_aspects(tmp.path()).unwrap();
        assert_eq!(spec.panels[0].aspect, Some(2.0));
        assert!(spec.preflight().is_empty());
    }
}
