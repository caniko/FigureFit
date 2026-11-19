//! Resolve the group hierarchy into flat, ordered panel sequences.
//!
//! The resolver walks the group tree and produces [`ResolvedSequence`]s —
//! one for each valid configuration (driven by `OneOf` groups and shared
//! panel assignments). Each sequence is a flat array of panels with
//! group boundary markers that the DP solver uses as mandatory split points.

use std::collections::{HashMap, HashSet};

use crate::config::{Direction, FigureSpec, GroupDef, GroupKind, PanelDef};
use crate::tree::SplitDir;

// ── Output types ────────────────────────────────────────────────────

/// A flattened, ordered panel sequence ready for the DP solver.
#[derive(Debug, Clone)]
pub struct ResolvedSequence {
    /// Panels in reading order.
    pub panels: Vec<ResolvedPanel>,
    /// Indices into `panels` where a group boundary exists.
    /// The solver MUST split at these positions.
    /// E.g. boundaries = [3, 5] means mandatory splits between
    /// panels[2]/panels[3] and panels[4]/panels[5].
    pub group_boundaries: Vec<usize>,
    /// Direction constraints for specific boundary positions.
    /// When a composite group has `direction = "row"` or `"column"`, its
    /// inter-child boundaries are constrained to only allow that split direction.
    pub boundary_directions: HashMap<usize, SplitDir>,
    /// Mapping from panel/group IDs to the panel index ranges they cover.
    /// Used for proximity scoring (a group's bbox = union of its panels' rects).
    pub id_spans: HashMap<String, (usize, usize)>,
    /// Leaf groups omitted by this variant before geometry solving.
    pub dropped_group_ids: HashSet<String>,
    /// Required groups that were omitted by this variant.
    pub unsatisfied_required: HashSet<String>,
    /// Required panels that were omitted by this variant.
    pub unsatisfied_required_panels: HashSet<String>,
}

/// A panel in the resolved sequence.
#[derive(Debug, Clone)]
pub struct ResolvedPanel {
    pub panel_id: String,
    pub group_id: String,
    pub aspect: f64,
    pub alt_label: Option<String>,
    pub min_width_frac: Option<f64>,
    pub max_width_frac: Option<f64>,
    pub min_height_frac: Option<f64>,
    pub required: bool,
}

// ── Resolver ────────────────────────────────────────────────────────

/// Resolve a figure spec into all valid panel sequences.
///
/// Multiple sequences arise from:
/// - `OneOf` groups (each child produces a variant)
/// - Shared panels (each assignment produces a variant)
/// - Panel alternatives (each alt aspect produces a variant)
///
/// Panels that are declared as insets of another panel are excluded from
/// the main sequences — they will be placed post-solve.
///
/// Results are capped at `max_variants` to prevent combinatorial explosion.
pub fn resolve(spec: &FigureSpec, max_variants: usize) -> Vec<ResolvedSequence> {
    let inset_ids = spec.inset_panel_ids();
    let panel_map = spec.panel_map();
    let group_map = spec.group_map();
    let Some(root_id) = spec.root_group_id() else {
        return Vec::new();
    };

    // Collect shared panels (panels appearing in more than one leaf group).
    let shared = find_shared_panels(spec);

    // Enumerate group configurations (OneOf selections + shared panel assignments).
    let configs = enumerate_group_configs(root_id, &group_map, &shared, max_variants);

    let mut sequences = Vec::new();
    let all_leaf_groups: HashSet<String> = spec
        .groups
        .iter()
        .filter(|g| matches!(g.kind, GroupKind::Atomic | GroupKind::Flexible))
        .map(|g| g.id.clone())
        .collect();
    let required_leaf_groups: HashSet<String> = spec
        .groups
        .iter()
        .filter(|g| matches!(g.kind, GroupKind::Atomic | GroupKind::Flexible) && g.required())
        .map(|g| g.id.clone())
        .collect();
    let required_panels = required_panel_ids(spec);

    for config in &configs {
        // For each group config, enumerate alt combinations.
        let alt_combos = enumerate_alt_combos(config, &panel_map, max_variants);

        for combo in alt_combos {
            if let Some(seq) = build_sequence(
                root_id,
                &group_map,
                &panel_map,
                &combo,
                config,
                &inset_ids,
                &all_leaf_groups,
                &required_leaf_groups,
                &required_panels,
            ) {
                sequences.push(seq);
            }
            if sequences.len() >= max_variants {
                return sequences;
            }
        }
    }

    sequences
}

// ── Shared panel detection ──────────────────────────────────────────

/// Find panels that appear in more than one leaf group.
fn find_shared_panels(spec: &FigureSpec) -> HashSet<String> {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for g in &spec.groups {
        if matches!(g.kind, GroupKind::Atomic | GroupKind::Flexible) {
            for pid in &g.panels {
                *seen.entry(pid.as_str()).or_default() += 1;
            }
        }
    }
    seen.into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(id, _)| id.to_string())
        .collect()
}

// ── Group configuration enumeration ─────────────────────────────────

/// A specific configuration: which groups are active, which panels are
/// assigned to which group (for shared panels).
#[derive(Debug, Clone)]
struct GroupConfig {
    /// Set of active group IDs.
    active_groups: HashSet<String>,
    /// For shared panels: panel_id → group_id that owns it in this config.
    panel_owner: HashMap<String, String>,
}

/// Enumerate valid group configurations.
fn enumerate_group_configs(
    root_id: &str,
    group_map: &HashMap<&str, &GroupDef>,
    shared: &HashSet<String>,
    max: usize,
) -> Vec<GroupConfig> {
    // Recursively collect all active-set variants (OneOf creates branches).
    let active_sets = collect_active_sets(root_id, group_map, max);

    let mut configs: Vec<GroupConfig> = active_sets
        .into_iter()
        .map(|active_groups| GroupConfig {
            active_groups,
            panel_owner: HashMap::new(),
        })
        .collect();

    if configs.is_empty() {
        return configs;
    }

    // For shared panels, expand configs with ownership assignments.
    if !shared.is_empty() {
        let mut expanded = Vec::new();
        for config in &configs {
            let assignments =
                enumerate_shared_assignments(shared, &config.active_groups, group_map);
            for assignment in assignments {
                let mut c = config.clone();
                c.panel_owner = assignment;
                expanded.push(c);
                if expanded.len() >= max {
                    return expanded;
                }
            }
        }
        if !expanded.is_empty() {
            return expanded;
        }
    }

    configs.truncate(max);
    configs
}

/// Recursively collect all valid active-group sets, branching at OneOf nodes.
/// Returns a list of possible active-group sets.
fn collect_active_sets(
    group_id: &str,
    group_map: &HashMap<&str, &GroupDef>,
    max: usize,
) -> Vec<HashSet<String>> {
    let Some(group) = group_map.get(group_id) else {
        return Vec::new();
    };

    match group.kind {
        GroupKind::Atomic | GroupKind::Flexible => {
            let mut s = HashSet::new();
            s.insert(group_id.to_string());
            vec![s]
        }
        GroupKind::Composite => {
            // Required children must be active. Optional children produce variants
            // where they are either omitted or included.
            let mut result = vec![{
                let mut s = HashSet::new();
                s.insert(group_id.to_string());
                s
            }];
            for child_id in &group.children {
                let child_required = group_map
                    .get(child_id.as_str())
                    .is_none_or(|g| g.required());
                let child_variants = collect_active_sets(child_id, group_map, max);
                let mut new_result = Vec::new();
                if !child_required {
                    new_result.extend(result.iter().cloned());
                }
                for existing in &result {
                    for cv in &child_variants {
                        let mut merged = existing.clone();
                        merged.extend(cv.iter().cloned());
                        new_result.push(merged);
                        if new_result.len() >= max {
                            return new_result;
                        }
                    }
                }
                result = new_result;
            }
            result
        }
        GroupKind::OneOf => {
            // Each child produces a separate variant.
            let mut result = Vec::new();
            for child_id in &group.children {
                let child_variants = collect_active_sets(child_id, group_map, max);
                for mut cv in child_variants {
                    cv.insert(group_id.to_string());
                    result.push(cv);
                    if result.len() >= max {
                        return result;
                    }
                }
            }
            result
        }
    }
}

/// For shared panels, enumerate which active leaf group owns each.
fn enumerate_shared_assignments(
    shared: &HashSet<String>,
    active_groups: &HashSet<String>,
    group_map: &HashMap<&str, &GroupDef>,
) -> Vec<HashMap<String, String>> {
    // For each shared panel, find which active leaf groups contain it.
    let mut panel_candidates: Vec<(String, Vec<String>)> = Vec::new();

    for panel_id in shared {
        let mut candidates = Vec::new();
        for gid in active_groups {
            if let Some(g) = group_map.get(gid.as_str())
                && matches!(g.kind, GroupKind::Atomic | GroupKind::Flexible)
                && g.panels.contains(panel_id)
            {
                candidates.push(gid.clone());
            }
        }
        if !candidates.is_empty() {
            panel_candidates.push((panel_id.clone(), candidates));
        }
    }

    // Cartesian product of all assignments.
    let mut result = vec![HashMap::new()];
    for (panel_id, candidates) in &panel_candidates {
        let mut new_result = Vec::new();
        for existing in &result {
            for candidate in candidates {
                let mut m = existing.clone();
                m.insert(panel_id.clone(), candidate.clone());
                new_result.push(m);
            }
        }
        result = new_result;
    }
    result
}

// ── Alt combination enumeration ─────────────────────────────────────

/// A specific aspect ratio choice for each panel.
#[derive(Debug, Clone)]
pub struct PanelChoice {
    pub panel_id: String,
    pub aspect: f64,
    pub alt_label: Option<String>,
}

/// Enumerate alt combinations for panels in a group config.
fn enumerate_alt_combos(
    config: &GroupConfig,
    panel_map: &HashMap<&str, &PanelDef>,
    max: usize,
) -> Vec<Vec<PanelChoice>> {
    // Collect all panels that are active in this config.
    let mut panel_ids: Vec<&str> = Vec::new();
    // We need the full panel list from the config, but we don't have
    // direct access to the panel ordering here. Collect from panel_map
    // keys that are active.
    for pid in panel_map.keys() {
        // A panel is active if it's not shared (and in some active group)
        // or it's shared and has an owner in this config.
        if config.panel_owner.contains_key(*pid) || !panel_ids.contains(pid) {
            panel_ids.push(pid);
        }
    }

    let mut combos: Vec<Vec<PanelChoice>> = vec![Vec::new()];

    for pid in &panel_ids {
        if let Some(panel) = panel_map.get(pid) {
            let candidates = panel.candidate_aspects();
            let mut new_combos = Vec::new();
            for combo in &combos {
                for (alt_label, aspect) in &candidates {
                    let mut c = combo.clone();
                    c.push(PanelChoice {
                        panel_id: pid.to_string(),
                        aspect: *aspect,
                        alt_label: alt_label.map(String::from),
                    });
                    new_combos.push(c);
                    if new_combos.len() >= max {
                        return new_combos;
                    }
                }
            }
            combos = new_combos;
        }
    }

    combos
}

// ── Sequence construction ───────────────────────────────────────────

/// Build a flat panel sequence from a group config and alt choices.
#[allow(clippy::too_many_arguments)]
fn build_sequence(
    root_id: &str,
    group_map: &HashMap<&str, &GroupDef>,
    panel_map: &HashMap<&str, &PanelDef>,
    choices: &[PanelChoice],
    config: &GroupConfig,
    inset_ids: &HashSet<String>,
    all_leaf_groups: &HashSet<String>,
    required_leaf_groups: &HashSet<String>,
    required_panels: &HashSet<String>,
) -> Option<ResolvedSequence> {
    let choice_map: HashMap<&str, &PanelChoice> =
        choices.iter().map(|c| (c.panel_id.as_str(), c)).collect();

    let mut panels = Vec::new();
    let mut boundaries = Vec::new();
    let mut boundary_directions: HashMap<usize, SplitDir> = HashMap::new();
    let mut id_spans: HashMap<String, (usize, usize)> = HashMap::new();
    let mut claimed: HashSet<String> = HashSet::new();

    flatten_group(
        root_id,
        group_map,
        panel_map,
        &choice_map,
        config,
        inset_ids,
        &mut panels,
        &mut boundaries,
        &mut boundary_directions,
        &mut id_spans,
        &mut claimed,
        true, // is root — no boundary before first child
        None, // no parent direction constraint
    )?;

    let placed_leaf_groups: HashSet<String> = panels.iter().map(|p| p.group_id.clone()).collect();
    let dropped_group_ids: HashSet<String> = all_leaf_groups
        .difference(&placed_leaf_groups)
        .cloned()
        .collect();
    let unsatisfied_required: HashSet<String> = required_leaf_groups
        .difference(&placed_leaf_groups)
        .cloned()
        .collect();
    let placed_panel_ids: HashSet<String> = panels
        .iter()
        .map(|p| p.panel_id.clone())
        .chain(inset_ids.iter().cloned())
        .collect();
    let unsatisfied_required_panels: HashSet<String> = required_panels
        .difference(&placed_panel_ids)
        .cloned()
        .collect();

    Some(ResolvedSequence {
        panels,
        group_boundaries: boundaries,
        boundary_directions,
        id_spans,
        dropped_group_ids,
        unsatisfied_required,
        unsatisfied_required_panels,
    })
}

fn required_panel_ids(spec: &FigureSpec) -> HashSet<String> {
    let panel_map = spec.panel_map();
    let mut required = HashSet::new();
    for group in &spec.groups {
        if !matches!(group.kind, GroupKind::Atomic | GroupKind::Flexible) {
            continue;
        }
        for pid in &group.panels {
            if panel_map
                .get(pid.as_str())
                .is_some_and(|p| p.required(group.priority))
            {
                required.insert(pid.clone());
            }
        }
    }
    required
}

/// Recursively flatten a group into the panel sequence.
#[allow(clippy::too_many_arguments)]
fn flatten_group(
    group_id: &str,
    group_map: &HashMap<&str, &GroupDef>,
    panel_map: &HashMap<&str, &PanelDef>,
    choice_map: &HashMap<&str, &PanelChoice>,
    config: &GroupConfig,
    inset_ids: &HashSet<String>,
    panels: &mut Vec<ResolvedPanel>,
    boundaries: &mut Vec<usize>,
    boundary_directions: &mut HashMap<usize, SplitDir>,
    id_spans: &mut HashMap<String, (usize, usize)>,
    claimed: &mut HashSet<String>,
    is_root: bool,
    // Direction constraint from parent composite, applied to boundary before this group.
    parent_dir: Option<SplitDir>,
) -> Option<()> {
    let group = group_map.get(group_id)?;

    if !config.active_groups.contains(group_id) {
        return Some(()); // group not active in this config
    }

    let start_idx = panels.len();

    // Add a group boundary before this group (unless it's the root
    // or the first item at the current position).
    if !is_root && !panels.is_empty() {
        let pos = panels.len();
        boundaries.push(pos);
        if let Some(dir) = parent_dir {
            boundary_directions.insert(pos, dir);
        }
    }

    match group.kind {
        GroupKind::Atomic => {
            for pid in &group.panels {
                // Skip inset panels — they are placed post-solve.
                if inset_ids.contains(pid.as_str()) {
                    continue;
                }
                // Skip if already claimed by another group (shared panel).
                if claimed.contains(pid) {
                    continue;
                }
                // Check if this group is the designated owner for shared panels.
                if let Some(owner) = config.panel_owner.get(pid.as_str())
                    && owner != group_id
                {
                    continue;
                }

                let pdef = panel_map.get(pid.as_str())?;
                let choice = choice_map.get(pid.as_str());
                let (aspect, alt_label) = match choice {
                    Some(c) => (c.aspect, c.alt_label.clone()),
                    None => (pdef.aspect?, None),
                };

                let panel_start = panels.len();
                panels.push(ResolvedPanel {
                    panel_id: pid.clone(),
                    group_id: group_id.to_string(),
                    aspect,
                    alt_label,
                    min_width_frac: pdef.min_width,
                    max_width_frac: pdef.max_width,
                    min_height_frac: pdef.min_height,
                    required: pdef.required(group.priority),
                });
                id_spans.insert(pid.clone(), (panel_start, panel_start + 1));
                claimed.insert(pid.clone());
            }
        }
        GroupKind::Flexible => {
            // Include panels that aren't claimed elsewhere.
            let mut included = 0;
            let min = group.min_panels.unwrap_or(1);
            let max = group.max_panels.unwrap_or(group.panels.len());
            for pid in &group.panels {
                // Skip inset panels — they are placed post-solve.
                if inset_ids.contains(pid.as_str()) {
                    continue;
                }
                if claimed.contains(pid) {
                    continue;
                }
                if let Some(owner) = config.panel_owner.get(pid.as_str())
                    && owner != group_id
                {
                    continue;
                }

                let pdef = panel_map.get(pid.as_str())?;
                let choice = choice_map.get(pid.as_str());
                let required = pdef.required(group.priority);
                if included >= max {
                    continue;
                }

                let (aspect, alt_label) = match choice {
                    Some(c) => (c.aspect, c.alt_label.clone()),
                    None => (pdef.aspect?, None),
                };

                let panel_start = panels.len();
                panels.push(ResolvedPanel {
                    panel_id: pid.clone(),
                    group_id: group_id.to_string(),
                    aspect,
                    alt_label,
                    min_width_frac: pdef.min_width,
                    max_width_frac: pdef.max_width,
                    min_height_frac: pdef.min_height,
                    required,
                });
                id_spans.insert(pid.clone(), (panel_start, panel_start + 1));
                claimed.insert(pid.clone());
                included += 1;
            }

            if included < min {
                return None; // can't meet minimum — invalid config
            }
        }
        GroupKind::Composite => {
            // Convert this group's direction to a SplitDir for child boundaries.
            let child_dir = group.direction.map(|d| match d {
                Direction::Row => SplitDir::Vertical,
                Direction::Column => SplitDir::Horizontal,
            });
            for (i, child_id) in group.children.iter().enumerate() {
                // First child inherits parent_dir; subsequent children get this
                // composite's direction constraint.
                let dir_for_child = if i == 0 { parent_dir } else { child_dir };
                flatten_group(
                    child_id,
                    group_map,
                    panel_map,
                    choice_map,
                    config,
                    inset_ids,
                    panels,
                    boundaries,
                    boundary_directions,
                    id_spans,
                    claimed,
                    i == 0 && is_root,
                    dir_for_child,
                )?;
            }
        }
        GroupKind::OneOf => {
            // In the current config, only one child should be active.
            // Find the first active child and flatten it.
            for child_id in &group.children {
                if config.active_groups.contains(child_id.as_str()) {
                    flatten_group(
                        child_id,
                        group_map,
                        panel_map,
                        choice_map,
                        config,
                        inset_ids,
                        panels,
                        boundaries,
                        boundary_directions,
                        id_spans,
                        claimed,
                        is_root,
                        parent_dir,
                    )?;
                    break;
                }
            }
        }
    }

    let end_idx = panels.len();
    if end_idx > start_idx {
        id_spans.insert(group_id.to_string(), (start_idx, end_idx));
    }

    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::FigureSpec;

    #[test]
    fn resolve_simple_hierarchy() {
        let toml = r#"
[page]
width_mm = 160
max_height_mm = 216

[figure]
name = "test"

[[panel]]
id = "A"
aspect = 1.0

[[panel]]
id = "B"
aspect = 1.5

[[panel]]
id = "C"
aspect = 1.2

[[group]]
id = "root"
kind = "composite"
children = ["g1", "g2"]

[[group]]
id = "g1"
kind = "atomic"
panels = ["A"]

[[group]]
id = "g2"
kind = "atomic"
panels = ["B", "C"]
"#;
        let spec = FigureSpec::from_toml(toml).unwrap();
        assert!(spec.preflight().is_empty());

        let seqs = resolve(&spec, 100);
        assert_eq!(seqs.len(), 1);

        let seq = &seqs[0];
        assert_eq!(seq.panels.len(), 3);
        assert_eq!(seq.panels[0].panel_id, "A");
        assert_eq!(seq.panels[1].panel_id, "B");
        assert_eq!(seq.panels[2].panel_id, "C");
        // Boundary between g1 and g2.
        assert_eq!(seq.group_boundaries, vec![1]);
        // id_spans should cover groups.
        assert_eq!(seq.id_spans["g1"], (0, 1));
        assert_eq!(seq.id_spans["g2"], (1, 3));
        assert_eq!(seq.id_spans["root"], (0, 3));
    }

    #[test]
    fn resolve_one_of() {
        let toml = r#"
[page]
width_mm = 160
max_height_mm = 216

[figure]
name = "test"

[[panel]]
id = "A"
aspect = 1.0

[[panel]]
id = "B"
aspect = 2.0

[[group]]
id = "root"
kind = "one_of"
children = ["opt1", "opt2"]

[[group]]
id = "opt1"
kind = "atomic"
panels = ["A"]

[[group]]
id = "opt2"
kind = "atomic"
panels = ["B"]
"#;
        let spec = FigureSpec::from_toml(toml).unwrap();
        let seqs = resolve(&spec, 100);
        // Should produce 2 variants: one with A, one with B.
        assert_eq!(seqs.len(), 2);
        assert_eq!(seqs[0].panels.len(), 1);
        assert_eq!(seqs[1].panels.len(), 1);

        let ids: HashSet<&str> = seqs.iter().map(|s| s.panels[0].panel_id.as_str()).collect();
        assert!(ids.contains("A"));
        assert!(ids.contains("B"));
    }

    #[test]
    fn resolve_shared_panel() {
        let toml = r#"
[page]
width_mm = 160
max_height_mm = 216

[figure]
name = "test"

[[panel]]
id = "A"
aspect = 1.0

[[panel]]
id = "E"
aspect = 1.5

[[panel]]
id = "F"
aspect = 0.8

[[group]]
id = "root"
kind = "composite"
children = ["g1", "g2"]

[[group]]
id = "g1"
kind = "flexible"
min_panels = 1
panels = ["A", "E"]

[[group]]
id = "g2"
kind = "flexible"
min_panels = 1
panels = ["E", "F"]
"#;
        let spec = FigureSpec::from_toml(toml).unwrap();
        assert!(spec.preflight().is_empty());

        let seqs = resolve(&spec, 100);
        // E is shared → 2 ownership variants (E in g1 or E in g2).
        assert!(seqs.len() >= 2);
        // In all variants, E should appear exactly once.
        for seq in &seqs {
            let e_count = seq.panels.iter().filter(|p| p.panel_id == "E").count();
            assert_eq!(e_count, 1, "E should appear exactly once");
        }
    }
}
