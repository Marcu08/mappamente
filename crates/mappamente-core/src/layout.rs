//! Deterministic layout engine.
//!
//! The engine turns the document tree into absolutely positioned, wrapped
//! boxes. It has no knowledge of the GUI: the same output is used by the
//! canvas, by SVG export and by headless rendering.
//!
//! Three algorithms are provided:
//!
//! - [`LayoutKind::Tree`] — bilateral mind map (branches alternate left/right);
//! - [`LayoutKind::Radial`] — branches spread around a full circle;
//! - [`LayoutKind::Logical`] — all branches grow to the right.
//!
//! Manual positions are stored as absolute top-left coordinates and translate
//! the whole subtree below the node, so dragging a branch keeps its shape.

use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, TAU};

use unicode_width::UnicodeWidthChar;

use crate::geom::{Point, Rect, Size};
use crate::model::{LayoutKind, MindMap, Node, NodeId};

/// Font and box metrics used to wrap labels and size nodes.
///
/// The GUI can tweak these values to match its own font; headless tools use the
/// defaults, which keeps their output deterministic.
#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    pub font_size: f32,
    pub line_height: f32,
    /// Width of one narrow (ASCII) character.
    pub char_width: f32,
    /// Maximum width of the text area inside a node.
    pub max_text_width: f32,
    pub pad_x: f32,
    pub pad_y: f32,
    pub min_width: f32,
    pub min_height: f32,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            font_size: 15.0,
            line_height: 20.0,
            char_width: 8.0,
            max_text_width: 240.0,
            pad_x: 14.0,
            pad_y: 9.0,
            min_width: 56.0,
            min_height: 34.0,
        }
    }
}

/// Distances between nodes for the automatic layouts.
#[derive(Clone, Copy, Debug)]
pub struct LayoutOptions {
    /// Horizontal gap between depth columns.
    pub horizontal_gap: f32,
    /// Vertical gap between sibling subtrees.
    pub vertical_gap: f32,
    /// Extra radial gap around the root and between rings.
    pub radial_gap: f32,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        Self {
            horizontal_gap: 60.0,
            vertical_gap: 16.0,
            radial_gap: 70.0,
        }
    }
}

/// Which side of the canvas a node lives on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Center,
    Left,
    Right,
}

/// A positioned node.
#[derive(Clone, Debug, PartialEq)]
pub struct LayoutNode {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub depth: u32,
    pub rect: Rect,
    /// Text already wrapped to the node width.
    pub lines: Vec<String>,
    pub side: Side,
    pub collapsed: bool,
}

/// Result of a layout pass.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub nodes: Vec<LayoutNode>,
    pub bounds: Rect,
    index: HashMap<NodeId, usize>,
}

impl Layout {
    /// Lays out `map` using its own [`LayoutKind`].
    #[must_use]
    pub fn compute(map: &MindMap, metrics: Metrics, options: LayoutOptions) -> Self {
        Self::compute_with(map, map.layout, metrics, options)
    }

    /// Lays out `map` with an explicit algorithm.
    #[must_use]
    pub fn compute_with(
        map: &MindMap,
        kind: LayoutKind,
        metrics: Metrics,
        options: LayoutOptions,
    ) -> Self {
        let mut root = MirrorNode::build(&map.root, &metrics);
        compute_heights(&mut root, options.vertical_gap);

        let mut nodes = match kind {
            LayoutKind::Tree => tree_nodes(&root, &options),
            LayoutKind::Radial => radial_nodes(&root, &options),
            LayoutKind::Logical => logical_nodes(&root, &options),
        };
        apply_manual_positions(map, &mut nodes);

        let mut index = HashMap::with_capacity(nodes.len());
        for (position, node) in nodes.iter().enumerate() {
            index.insert(node.id.clone(), position);
        }

        Self {
            bounds: bounds_of(&nodes),
            nodes,
            index,
        }
    }

    /// Looks up a positioned node by id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&LayoutNode> {
        self.index.get(id).map(|position| &self.nodes[*position])
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

struct MirrorNode {
    id: NodeId,
    lines: Vec<String>,
    size: Size,
    children: Vec<MirrorNode>,
    collapsed: bool,
    /// Height of the whole subtree box, filled by [`compute_heights`].
    height: f32,
}

impl MirrorNode {
    fn build(node: &Node, metrics: &Metrics) -> Self {
        let lines = wrap_text(&node.text, metrics);
        let size = node_size(&lines, metrics);
        let children = if node.collapsed {
            Vec::new()
        } else {
            node.children
                .iter()
                .map(|child| Self::build(child, metrics))
                .collect()
        };
        Self {
            id: node.id.clone(),
            lines,
            size,
            children,
            collapsed: node.collapsed,
            height: size.h,
        }
    }
}

/// Wraps a label to [`Metrics::max_text_width`], preserving explicit newlines.
#[must_use]
pub fn wrap_text(text: &str, metrics: &Metrics) -> Vec<String> {
    let max = metrics.max_text_width.max(metrics.char_width * 4.0);
    let mut lines = Vec::new();

    for paragraph in text.split('\n') {
        let mut line = String::new();
        let mut line_width = 0.0_f32;
        let mut has_word = false;

        for word in paragraph.split_whitespace() {
            let word_width = text_width(word, metrics);
            let space_width = if has_word { metrics.char_width } else { 0.0 };

            if has_word && line_width + space_width + word_width > max {
                lines.push(std::mem::take(&mut line));
                line_width = 0.0;
                has_word = false;
            }

            if has_word {
                line.push(' ');
                line.push_str(word);
                line_width += metrics.char_width + word_width;
            } else if word_width > max {
                // A single word wider than the node: split it by character.
                let mut chunk = String::new();
                let mut chunk_width = 0.0_f32;
                for ch in word.chars() {
                    let width = char_width(ch, metrics);
                    if !chunk.is_empty() && chunk_width + width > max {
                        lines.push(std::mem::take(&mut chunk));
                        chunk_width = 0.0;
                    }
                    chunk.push(ch);
                    chunk_width += width;
                }
                line = chunk;
                line_width = chunk_width;
                has_word = !line.is_empty();
            } else {
                line.push_str(word);
                line_width = word_width;
                has_word = true;
            }
        }

        if has_word {
            lines.push(line);
        } else {
            lines.push(String::new());
        }
    }

    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn char_width(ch: char, metrics: &Metrics) -> f32 {
    match UnicodeWidthChar::width(ch).unwrap_or(0) {
        0 => 0.0,
        1 => metrics.char_width,
        _ => metrics.char_width * 2.0,
    }
}

fn text_width(text: &str, metrics: &Metrics) -> f32 {
    let mut width = 0.0_f32;
    for ch in text.chars() {
        width += char_width(ch, metrics);
    }
    width
}

fn node_size(lines: &[String], metrics: &Metrics) -> Size {
    let mut label_width = 0.0_f32;
    for line in lines {
        label_width = label_width.max(text_width(line, metrics));
    }
    let width = (label_width + 2.0 * metrics.pad_x).max(metrics.min_width);
    let height =
        (lines.len() as f32 * metrics.line_height + 2.0 * metrics.pad_y).max(metrics.min_height);
    Size::new(width, height)
}

fn compute_heights(node: &mut MirrorNode, gap: f32) {
    let count = node.children.len();
    let mut total = 0.0_f32;
    for (index, child) in node.children.iter_mut().enumerate() {
        compute_heights(child, gap);
        total += child.height;
        if index + 1 < count {
            total += gap;
        }
    }
    node.height = node.size.h.max(total);
}

fn child_total(node: &MirrorNode, gap: f32) -> f32 {
    let count = node.children.len();
    if count == 0 {
        return 0.0;
    }
    node.children.iter().map(|child| child.height).sum::<f32>() + gap * (count - 1) as f32
}

fn count_visible(node: &MirrorNode) -> usize {
    1 + node.children.iter().map(count_visible).sum::<usize>()
}

fn layout_node(
    node: &MirrorNode,
    depth: usize,
    rect: Rect,
    parent: Option<&NodeId>,
    side: Side,
) -> LayoutNode {
    LayoutNode {
        id: node.id.clone(),
        parent: parent.cloned(),
        depth: depth as u32,
        rect,
        lines: node.lines.clone(),
        side,
        collapsed: node.collapsed,
    }
}

// ---------------------------------------------------------------------------
// Logical (all branches to the right)
// ---------------------------------------------------------------------------

fn logical_nodes(root: &MirrorNode, options: &LayoutOptions) -> Vec<LayoutNode> {
    let mut widths = Vec::new();
    collect_widths(root, 0, &mut widths);
    let mut offsets = vec![0.0_f32; widths.len()];
    for depth in 1..widths.len() {
        offsets[depth] = offsets[depth - 1] + widths[depth - 1] + options.horizontal_gap;
    }

    let mut out = Vec::with_capacity(count_visible(root));
    let root_y = (root.height - root.size.h) / 2.0;
    out.push(layout_node(
        root,
        0,
        Rect::new(offsets[0], root_y, root.size.w, root.size.h),
        None,
        Side::Center,
    ));

    if !root.children.is_empty() {
        let total = child_total(root, options.vertical_gap);
        let mut top = root_y + root.size.h / 2.0 - total / 2.0;
        let placement = Placement {
            offsets: &offsets,
            sibling_gap: options.vertical_gap,
            branch_offset: 0.0,
            column_base: 1,
            side: Side::Right,
        };
        for child in &root.children {
            place_branch(child, 0, top, &placement, Some(&root.id), &mut out);
            top += child.height + options.vertical_gap;
        }
    }
    out
}

fn collect_widths(node: &MirrorNode, depth: usize, widths: &mut Vec<f32>) {
    if widths.len() <= depth {
        widths.push(0.0);
    }
    widths[depth] = widths[depth].max(node.size.w);
    for child in &node.children {
        collect_widths(child, depth + 1, widths);
    }
}

struct Placement<'a> {
    offsets: &'a [f32],
    sibling_gap: f32,
    /// Distance from the origin to the first column of this branch.
    branch_offset: f32,
    /// Index in `offsets` used by the first column of this branch.
    column_base: usize,
    side: Side,
}

fn place_branch(
    node: &MirrorNode,
    level: usize,
    top: f32,
    placement: &Placement<'_>,
    parent: Option<&NodeId>,
    out: &mut Vec<LayoutNode>,
) {
    let y = top + (node.height - node.size.h) / 2.0;
    let column = placement.column_base + level;
    let x = match placement.side {
        Side::Right => placement.branch_offset + placement.offsets[column],
        Side::Left => -placement.branch_offset - placement.offsets[column] - node.size.w,
        Side::Center => placement.offsets[column],
    };
    // Column 0 of a side contains the direct children of the root, which are
    // at depth 1 (the root itself is depth 0).
    out.push(layout_node(
        node,
        level + 1,
        Rect::new(x, y, node.size.w, node.size.h),
        parent,
        placement.side,
    ));

    if !node.children.is_empty() {
        let total = child_total(node, placement.sibling_gap);
        let mut cursor = y + node.size.h / 2.0 - total / 2.0;
        for child in &node.children {
            place_branch(child, level + 1, cursor, placement, Some(&node.id), out);
            cursor += child.height + placement.sibling_gap;
        }
    }
}

// ---------------------------------------------------------------------------
// Bilateral tree
// ---------------------------------------------------------------------------

fn tree_nodes(root: &MirrorNode, options: &LayoutOptions) -> Vec<LayoutNode> {
    let mut out = Vec::with_capacity(count_visible(root));
    let root_rect = Rect::new(
        -root.size.w / 2.0,
        -root.size.h / 2.0,
        root.size.w,
        root.size.h,
    );
    out.push(layout_node(root, 0, root_rect, None, Side::Center));

    let right: Vec<&MirrorNode> = root.children.iter().step_by(2).collect();
    let left: Vec<&MirrorNode> = root.children.iter().skip(1).step_by(2).collect();
    let root_half_w = root.size.w / 2.0;

    place_side(
        &right,
        root_half_w,
        options,
        Side::Right,
        &root.id,
        &mut out,
    );
    place_side(&left, root_half_w, options, Side::Left, &root.id, &mut out);
    out
}

fn place_side(
    nodes: &[&MirrorNode],
    root_half_w: f32,
    options: &LayoutOptions,
    side: Side,
    root_id: &NodeId,
    out: &mut Vec<LayoutNode>,
) {
    if nodes.is_empty() {
        return;
    }

    let mut widths = Vec::new();
    for node in nodes {
        collect_widths(node, 0, &mut widths);
    }
    let mut offsets = vec![0.0_f32; widths.len()];
    for level in 1..widths.len() {
        offsets[level] = offsets[level - 1] + widths[level - 1] + options.horizontal_gap;
    }

    let total = nodes.iter().map(|node| node.height).sum::<f32>()
        + options.vertical_gap * (nodes.len() - 1) as f32;
    let mut top = -total / 2.0;
    let placement = Placement {
        offsets: &offsets,
        sibling_gap: options.vertical_gap,
        branch_offset: root_half_w + options.horizontal_gap,
        column_base: 0,
        side,
    };
    for node in nodes {
        place_branch(node, 0, top, &placement, Some(root_id), out);
        top += node.height + options.vertical_gap;
    }
}

// ---------------------------------------------------------------------------
// Radial
// ---------------------------------------------------------------------------

struct RadialEntry {
    id: NodeId,
    parent: Option<NodeId>,
    depth: u32,
    angle: f32,
    size: Size,
    lines: Vec<String>,
    collapsed: bool,
    half_diagonal: f32,
}

fn radial_entry(node: &MirrorNode, depth: u32, angle: f32, parent: Option<&NodeId>) -> RadialEntry {
    RadialEntry {
        id: node.id.clone(),
        parent: parent.cloned(),
        depth,
        angle,
        size: node.size,
        lines: node.lines.clone(),
        collapsed: node.collapsed,
        half_diagonal: (node.size.w * node.size.w + node.size.h * node.size.h).sqrt() / 2.0,
    }
}

fn leaf_weight(node: &MirrorNode) -> usize {
    if node.children.is_empty() {
        1
    } else {
        node.children.iter().map(leaf_weight).sum()
    }
}

fn radial_entries(root: &MirrorNode) -> Vec<RadialEntry> {
    let mut out = Vec::with_capacity(count_visible(root));
    out.push(radial_entry(root, 0, 0.0, None));

    if !root.children.is_empty() {
        let total = root.children.iter().map(leaf_weight).sum::<usize>();
        let mut cursor = -FRAC_PI_2;
        for child in &root.children {
            let span = TAU * leaf_weight(child) as f32 / total as f32;
            let angle = cursor + span / 2.0;
            push_radial(child, 1, angle, span, &root.id, &mut out);
            cursor += span;
        }
    }
    out
}

fn push_radial(
    node: &MirrorNode,
    depth: u32,
    angle: f32,
    span: f32,
    parent: &NodeId,
    out: &mut Vec<RadialEntry>,
) {
    out.push(radial_entry(node, depth, angle, Some(parent)));
    if node.children.is_empty() {
        return;
    }

    let total = node.children.iter().map(leaf_weight).sum::<usize>();
    let mut cursor = angle - span / 2.0;
    for child in &node.children {
        let child_span = span * leaf_weight(child) as f32 / total as f32;
        let child_angle = cursor + child_span / 2.0;
        push_radial(child, depth + 1, child_angle, child_span, &node.id, out);
        cursor += child_span;
    }
}

fn radial_nodes(root: &MirrorNode, options: &LayoutOptions) -> Vec<LayoutNode> {
    let entries = radial_entries(root);
    let max_depth = entries
        .iter()
        .map(|entry| entry.depth as usize)
        .max()
        .unwrap_or(0);

    let mut max_half_diagonal = vec![0.0_f32; max_depth + 1];
    for entry in &entries {
        let depth = entry.depth as usize;
        max_half_diagonal[depth] = max_half_diagonal[depth].max(entry.half_diagonal);
    }

    // Nodes of the same depth sit on a circle. Its radius is large enough that
    // their circumcircles never touch, so text boxes cannot overlap.
    let mut radii = vec![0.0_f32; max_depth + 1];
    for depth in 1..=max_depth {
        let mut radius = radii[depth - 1]
            + max_half_diagonal[depth - 1]
            + max_half_diagonal[depth]
            + options.radial_gap;

        let mut level: Vec<&RadialEntry> = entries
            .iter()
            .filter(|entry| entry.depth as usize == depth)
            .collect();
        level.sort_by(|a, b| a.angle.total_cmp(&b.angle));

        let count = level.len();
        if count > 1 {
            for index in 0..count {
                let current = level[index];
                let next = level[(index + 1) % count];
                let delta = if index + 1 == count {
                    next.angle + TAU - current.angle
                } else {
                    next.angle - current.angle
                };
                if delta > 1e-4 {
                    let required =
                        (current.half_diagonal + next.half_diagonal + options.radial_gap)
                            / (2.0 * (delta / 2.0).sin());
                    radius = radius.max(required);
                }
            }
        }
        radii[depth] = radius;
    }

    entries
        .iter()
        .map(|entry| {
            let depth = entry.depth as usize;
            let center = if depth == 0 {
                Point::ZERO
            } else {
                Point::new(
                    radii[depth] * entry.angle.cos(),
                    radii[depth] * entry.angle.sin(),
                )
            };
            let side = if depth == 0 {
                Side::Center
            } else if entry.angle.cos() >= 0.0 {
                Side::Right
            } else {
                Side::Left
            };
            LayoutNode {
                id: entry.id.clone(),
                parent: entry.parent.clone(),
                depth: entry.depth,
                rect: Rect::from_center(center, entry.size),
                lines: entry.lines.clone(),
                side,
                collapsed: entry.collapsed,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Manual overrides
// ---------------------------------------------------------------------------

fn apply_manual_positions(map: &MindMap, nodes: &mut [LayoutNode]) {
    let auto: HashMap<String, Point> = nodes
        .iter()
        .map(|node| (node.id.clone(), node.rect.origin()))
        .collect();
    let mut deltas = HashMap::with_capacity(nodes.len());
    collect_manual_deltas(&map.root, Point::ZERO, &auto, &mut deltas);

    for node in nodes.iter_mut() {
        if let Some(delta) = deltas.get(&node.id) {
            node.rect.x += delta.x;
            node.rect.y += delta.y;
        }
    }
}

fn collect_manual_deltas(
    node: &Node,
    inherited: Point,
    auto: &HashMap<String, Point>,
    deltas: &mut HashMap<String, Point>,
) {
    let delta = match node.pos {
        Some([x, y]) => match auto.get(&node.id) {
            // Absolute override: ignore the inherited shift and land exactly
            // where the file says.
            Some(origin) => Point::new(x - origin.x, y - origin.y),
            None => inherited,
        },
        None => inherited,
    };
    deltas.insert(node.id.clone(), delta);

    if !node.collapsed {
        for child in &node.children {
            collect_manual_deltas(child, delta, auto, deltas);
        }
    }
}

fn bounds_of(nodes: &[LayoutNode]) -> Rect {
    let mut bounds: Option<Rect> = None;
    for node in nodes {
        bounds = Some(match bounds {
            Some(current) => current.union(node.rect),
            None => node.rect,
        });
    }
    bounds.unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MindMap;

    fn add_levels(map: &mut MindMap, parent: &str, depth: usize, breadth: usize, level: usize) {
        if level >= depth {
            return;
        }
        for index in 0..breadth {
            let text = format!(
                "Node {level}-{index} {}",
                "text ".repeat((level + index) % 3)
            );
            let id = map.add_child(parent, text).unwrap();
            add_levels(map, &id, depth, breadth, level + 1);
        }
    }

    fn sample(kind: LayoutKind) -> MindMap {
        let mut map = MindMap::new("Layout", "Root");
        map.set_layout(kind);
        let root = map.root.id.clone();
        add_levels(&mut map, &root, 4, 3, 0);
        map
    }

    fn compute(map: &MindMap) -> Layout {
        Layout::compute(map, Metrics::default(), LayoutOptions::default())
    }

    fn assert_no_overlaps(layout: &Layout) {
        for (index, first) in layout.nodes.iter().enumerate() {
            for second in &layout.nodes[index + 1..] {
                assert!(
                    !first.rect.overlaps(second.rect, 0.01),
                    "nodes {} {:?} and {} {:?} overlap",
                    first.id,
                    first.rect,
                    second.id,
                    second.rect
                );
            }
        }
    }

    #[test]
    fn layouts_do_not_overlap() {
        for kind in [LayoutKind::Tree, LayoutKind::Radial, LayoutKind::Logical] {
            let map = sample(kind);
            let layout = compute(&map);
            assert_eq!(layout.len(), map.node_count());
            assert_no_overlaps(&layout);
        }
    }

    #[test]
    fn layout_is_deterministic() {
        let map = sample(LayoutKind::Radial);
        let first = compute(&map);
        let second = compute(&map);
        assert_eq!(first, second);
    }

    #[test]
    fn bounds_contain_all_nodes() {
        for kind in [LayoutKind::Tree, LayoutKind::Radial, LayoutKind::Logical] {
            let map = sample(kind);
            let layout = compute(&map);
            for node in &layout.nodes {
                let bounds = layout.bounds;
                assert!(node.rect.x >= bounds.x - 0.01);
                assert!(node.rect.y >= bounds.y - 0.01);
                assert!(node.rect.right() <= bounds.right() + 0.01);
                assert!(node.rect.bottom() <= bounds.bottom() + 0.01);
            }
        }
    }

    #[test]
    fn radial_root_is_centered() {
        let map = sample(LayoutKind::Radial);
        let layout = compute(&map);
        let root = layout.get(&map.root.id).unwrap();
        assert!(root.rect.center().x.abs() < 0.01);
        assert!(root.rect.center().y.abs() < 0.01);
    }

    #[test]
    fn radial_ring_distances_grow_with_depth() {
        let map = sample(LayoutKind::Radial);
        let layout = compute(&map);
        let mut radius_by_depth: HashMap<u32, f32> = HashMap::new();
        for node in &layout.nodes {
            if node.depth == 0 {
                continue;
            }
            let center = node.rect.center();
            let radius = (center.x * center.x + center.y * center.y).sqrt();
            radius_by_depth
                .entry(node.depth)
                .and_modify(|value| assert!((*value - radius).abs() < 0.05))
                .or_insert(radius);
        }
        for depth in 2..=3 {
            let previous = radius_by_depth[&(depth - 1)];
            let current = radius_by_depth[&depth];
            assert!(current > previous + 1.0, "rings must grow with depth");
        }
    }

    #[test]
    fn logical_children_are_to_the_right() {
        let map = sample(LayoutKind::Logical);
        let layout = compute(&map);
        for node in &layout.nodes {
            if let Some(parent_id) = &node.parent {
                let parent = layout.get(parent_id).unwrap();
                assert!(
                    node.rect.x >= parent.rect.right(),
                    "child {} must start after parent {}",
                    node.id,
                    parent.id
                );
            }
        }
    }

    #[test]
    fn tree_has_branches_on_both_sides() {
        let map = sample(LayoutKind::Tree);
        let layout = compute(&map);
        let root = layout.get(&map.root.id).unwrap();
        let left = layout
            .nodes
            .iter()
            .filter(|node| node.rect.center().x < root.rect.center().x)
            .count();
        let right = layout
            .nodes
            .iter()
            .filter(|node| node.rect.center().x > root.rect.center().x)
            .count();
        assert!(left > 1, "expected a left branch, got {left}");
        assert!(right > 1, "expected a right branch, got {right}");
    }

    #[test]
    fn layout_depths_match_the_document() {
        for kind in [LayoutKind::Tree, LayoutKind::Radial, LayoutKind::Logical] {
            let map = sample(kind);
            let layout = compute(&map);
            for node in &layout.nodes {
                assert_eq!(
                    node.depth as usize,
                    map.depth_of(&node.id).unwrap(),
                    "wrong depth for {} in {kind:?}",
                    node.id
                );
            }
        }
    }

    #[test]
    fn collapsed_children_are_hidden() {
        let mut map = sample(LayoutKind::Tree);
        let branch = map.root.children[0].id.clone();
        let hidden = map.find(&branch).unwrap().node_count() - 1;
        map.set_collapsed(&branch, true).unwrap();
        let layout = compute(&map);
        assert_eq!(layout.len(), map.node_count() - hidden);
        assert!(layout.get(&branch).is_some());
        for node in map.find(&branch).unwrap().children.clone() {
            assert!(layout.get(&node.id).is_none());
        }
    }

    #[test]
    fn manual_position_translates_the_whole_subtree() {
        let mut map = sample(LayoutKind::Logical);
        let branch = map.root.children[0].id.clone();
        let grandchild = map.find(&branch).unwrap().children[0].id.clone();
        let sibling = map.root.children[1].id.clone();

        let before = compute(&map);
        let branch_rect = before.get(&branch).unwrap().rect;
        let grandchild_rect = before.get(&grandchild).unwrap().rect;
        let sibling_rect = before.get(&sibling).unwrap().rect;

        let delta = [120.0, -80.0];
        map.set_pos(
            &branch,
            Some([branch_rect.x + delta[0], branch_rect.y + delta[1]]),
        )
        .unwrap();
        let after = compute(&map);

        let moved = after.get(&branch).unwrap().rect;
        assert!((moved.x - (branch_rect.x + delta[0])).abs() < 0.01);
        assert!((moved.y - (branch_rect.y + delta[1])).abs() < 0.01);

        let moved_child = after.get(&grandchild).unwrap().rect;
        assert!((moved_child.x - (grandchild_rect.x + delta[0])).abs() < 0.01);
        assert!((moved_child.y - (grandchild_rect.y + delta[1])).abs() < 0.01);

        let untouched = after.get(&sibling).unwrap().rect;
        assert_eq!(untouched, sibling_rect);
    }

    #[test]
    fn wrapping_limits_node_width_and_grows_height() {
        let mut map = MindMap::new("Wrap", "Root");
        let long = map
            .add_child(&map.root.id.clone(), "word ".repeat(80))
            .unwrap();
        let metrics = Metrics::default();
        let layout = Layout::compute(&map, metrics, LayoutOptions::default());
        let node = layout.get(&long).unwrap();
        let max_width = metrics.max_text_width + 2.0 * metrics.pad_x + 0.01;
        assert!(node.rect.w <= max_width, "width was {}", node.rect.w);
        assert!(node.lines.len() > 1);
        assert!(
            node.rect.h > metrics.line_height + 2.0 * metrics.pad_y,
            "height was {}",
            node.rect.h
        );
    }

    #[test]
    fn explicit_newlines_are_kept() {
        let metrics = Metrics::default();
        let lines = wrap_text("first\nsecond", &metrics);
        assert_eq!(lines, vec!["first".to_owned(), "second".to_owned()]);
    }

    #[test]
    fn empty_labels_get_minimum_size() {
        let mut map = MindMap::new("Empty", "");
        map.add_child(&map.root.id.clone(), "").unwrap();
        let metrics = Metrics::default();
        let layout = Layout::compute(&map, metrics, LayoutOptions::default());
        for node in &layout.nodes {
            assert!(node.rect.w >= metrics.min_width);
            assert!(node.rect.h >= metrics.min_height);
        }
    }

    #[test]
    fn wide_characters_count_double() {
        let metrics = Metrics::default();
        let latin = wrap_text("abcd", &metrics);
        let wide = wrap_text("地图文字", &metrics);
        assert_eq!(latin.len(), 1);
        assert_eq!(wide.len(), 1);

        let mut map = MindMap::new("Wide", "Root");
        let a = map.add_child(&map.root.id.clone(), "abcd").unwrap();
        let b = map.add_child(&map.root.id.clone(), "地图文字").unwrap();
        let layout = Layout::compute(&map, metrics, LayoutOptions::default());
        assert!(layout.get(&b).unwrap().rect.w > layout.get(&a).unwrap().rect.w);
    }
}
