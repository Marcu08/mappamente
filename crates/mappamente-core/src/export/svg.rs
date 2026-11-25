//! SVG export.
//!
//! The renderer uses the same deterministic layout and [palettes](crate::theme)
//! as the GUI, so an exported file looks like the editor canvas. Output is a
//! standalone SVG with a background, soft node shadows, curved edges and
//! wrapped labels.

use std::collections::HashMap;
use std::fmt::Write as _;

use crate::geom::Point;
use crate::layout::{Layout, LayoutOptions, Metrics};
use crate::model::{LayoutKind, MindMap, Node};
use crate::theme::{Palette, mix_hex, parse_hex, readable_text};

/// Knobs for [`to_svg`].
#[derive(Clone, Debug)]
pub struct SvgOptions {
    /// Overrides the layout stored in the map.
    pub layout: Option<LayoutKind>,
    /// Overrides the theme stored in the map.
    pub theme: Option<String>,
    /// Padding around the drawing.
    pub margin: f32,
    pub metrics: Metrics,
    pub layout_options: LayoutOptions,
}

impl Default for SvgOptions {
    fn default() -> Self {
        Self {
            layout: None,
            theme: None,
            margin: 40.0,
            metrics: Metrics::default(),
            layout_options: LayoutOptions::default(),
        }
    }
}

/// Renders a complete standalone SVG document.
#[must_use]
pub fn to_svg(map: &MindMap, options: &SvgOptions) -> String {
    let kind = options.layout.unwrap_or(map.layout);
    let layout = Layout::compute_with(map, kind, options.metrics, options.layout_options);
    render(map, &layout, options)
}

fn render(map: &MindMap, layout: &Layout, options: &SvgOptions) -> String {
    let palette = Palette::by_name(options.theme.as_deref().unwrap_or(&map.theme));
    let margin = options.margin.max(0.0);
    let bounds = layout.bounds;
    let width = (bounds.w + margin * 2.0).max(64.0);
    let height = (bounds.h + margin * 2.0).max(64.0);
    let offset_x = margin - bounds.x;
    let offset_y = margin - bounds.y;

    let nodes_by_id = index_nodes(map);
    let branches = branch_indices(map);
    let branch_fills = palette.branch_fills();
    let mut svg = String::with_capacity(4096 + layout.nodes.len() * 320);
    let _ = writeln!(svg, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    let _ = writeln!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width:.1}" height="{height:.1}" viewBox="0 0 {width:.1} {height:.1}" role="img">"#
    );
    let _ = writeln!(svg, "<title>{}</title>", escape_xml(&map.title));
    let _ = writeln!(
        svg,
        r##"<defs><filter id="mappamente-shadow" x="-20%" y="-20%" width="140%" height="140%"><feDropShadow dx="0" dy="3" stdDeviation="5" flood-color="#000000" flood-opacity="0.12"/></filter></defs>"##
    );
    let _ = writeln!(
        svg,
        r#"<rect width="100%" height="100%" fill="{}"/>"#,
        palette.background
    );

    // Edges first, so nodes cover their endpoints.
    let _ = writeln!(
        svg,
        r#"<g fill="none" stroke-linecap="round" stroke-linejoin="round">"#
    );
    for node in &layout.nodes {
        let Some(parent_id) = &node.parent else {
            continue;
        };
        let Some(parent) = layout.get(parent_id) else {
            continue;
        };
        let from = parent.rect.edge_toward(node.rect.center());
        let to = node.rect.edge_toward(parent.rect.center());
        let (c1, c2) = control_points(from, to);
        let stroke_width = (2.6 - 0.22 * node.depth as f32).max(1.1);
        let edge = branches
            .get(node.id.as_str())
            .map_or(palette.edge, |branch| {
                branch_fills[*branch % branch_fills.len()]
            });
        let _ = writeln!(
            svg,
            r##"<path d="M {:.1} {:.1} C {:.1} {:.1}, {:.1} {:.1}, {:.1} {:.1}" stroke="{edge}" stroke-opacity="0.82" stroke-width="{stroke_width:.1}"/>"##,
            from.x + offset_x,
            from.y + offset_y,
            c1.x + offset_x,
            c1.y + offset_y,
            c2.x + offset_x,
            c2.y + offset_y,
            to.x + offset_x,
            to.y + offset_y,
        );
    }
    let _ = writeln!(svg, "</g>");

    for node in &layout.nodes {
        let model_node = nodes_by_id.get(node.id.as_str()).copied();
        let custom_color = model_node
            .and_then(|model_node| model_node.color.as_deref())
            .filter(|color| parse_hex(color).is_some());
        let branch = branches.get(node.id.as_str()).copied();
        let fill = if let Some(custom) = custom_color {
            custom.to_owned()
        } else if node.depth == 0 {
            palette.accent.to_owned()
        } else if let Some(branch) = branch {
            let base = branch_fills[branch % branch_fills.len()];
            if node.depth <= 1 {
                base.to_owned()
            } else {
                let factor = ((node.depth - 1) as f32 * 0.18).min(if palette.is_dark() {
                    0.55
                } else {
                    0.68
                });
                mix_hex(base, palette.toolbar, factor).unwrap_or_else(|| base.to_owned())
            }
        } else {
            palette.node_fill(node.depth).to_owned()
        };
        let text_color = if node.depth == 0 && custom_color.is_none() {
            palette.accent_text
        } else {
            readable_text(&fill)
        };
        let stroke = mix_hex(&fill, palette.text, 0.16).unwrap_or_else(|| palette.edge.to_owned());
        let expand = if node.depth == 0 { 1.5 } else { 0.0 };

        let x = node.rect.x + offset_x - expand;
        let y = node.rect.y + offset_y - expand;
        let width = node.rect.w + expand * 2.0;
        let height = node.rect.h + expand * 2.0;
        let radius = (height * 0.5).min(18.0);
        let _ = writeln!(svg, "<g>");
        let _ = writeln!(
            svg,
            r##"<rect x="{x:.1}" y="{y:.1}" width="{width:.1}" height="{height:.1}" rx="{radius:.1}" fill="{fill}" stroke="{stroke}" stroke-width="1" filter="url(#mappamente-shadow)"/>"##
        );

        let font_size = options.metrics.font_size;
        let line_height = options.metrics.line_height;
        for (index, line) in node.lines.iter().enumerate() {
            if line.is_empty() {
                continue;
            }
            let baseline = y + options.metrics.pad_y + (index as f32 + 0.78) * line_height;
            let _ = writeln!(
                svg,
                r#"<text x="{:.1}" y="{baseline:.1}" font-family="Inter, 'Segoe UI', system-ui, sans-serif" font-size="{font_size:.1}" fill="{text_color}" xml:space="preserve">{}</text>"#,
                node.rect.x + offset_x + options.metrics.pad_x,
                escape_xml(line),
            );
        }

        if node.collapsed {
            let dot = mix_hex(&fill, text_color, 0.75).unwrap_or_else(|| text_color.to_owned());
            let _ = writeln!(
                svg,
                r#"<circle cx="{:.1}" cy="{:.1}" r="4" fill="{dot}" fill-opacity="0.75"/>"#,
                node.rect.x + node.rect.w - 10.0 + offset_x,
                y + node.rect.h / 2.0,
            );
        }
        let _ = writeln!(svg, "</g>");
    }

    let _ = writeln!(svg, "</svg>");
    svg
}

fn index_nodes(map: &MindMap) -> HashMap<&str, &Node> {
    fn walk<'a>(node: &'a Node, out: &mut HashMap<&'a str, &'a Node>) {
        out.insert(node.id.as_str(), node);
        for child in &node.children {
            walk(child, out);
        }
    }

    let mut out = HashMap::with_capacity(map.node_count());
    walk(&map.root, &mut out);
    out
}

fn branch_indices(map: &MindMap) -> HashMap<&str, usize> {
    fn walk<'a>(node: &'a Node, branch: usize, out: &mut HashMap<&'a str, usize>) {
        for child in &node.children {
            out.insert(child.id.as_str(), branch);
            walk(child, branch, out);
        }
    }

    let mut out = HashMap::with_capacity(map.node_count());
    for (branch, child) in map.root.children.iter().enumerate() {
        out.insert(child.id.as_str(), branch);
        walk(child, branch, &mut out);
    }
    out
}

fn control_points(from: Point, to: Point) -> (Point, Point) {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    if dx.abs() >= dy.abs() {
        (
            Point::new(from.x + dx * 0.5, from.y),
            Point::new(to.x - dx * 0.5, to.y),
        )
    } else {
        (
            Point::new(from.x, from.y + dy * 0.5),
            Point::new(to.x, to.y - dy * 0.5),
        )
    }
}

fn escape_xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LayoutKind;

    fn sample() -> MindMap {
        let mut map = MindMap::new("Demo & <test>", "Root \"quoted\"");
        let root = map.root.id.clone();
        let child = map.add_child(&root, "A & B <tag>").unwrap();
        map.add_child(&child, "Deep \"node\"").unwrap();
        map.set_color(&child, Some("#ffcc00")).unwrap();
        map.set_layout(LayoutKind::Tree);
        map.theme = "light".to_owned();
        map
    }

    #[test]
    fn output_is_well_formed_xml() {
        let svg = to_svg(&sample(), &SvgOptions::default());
        let document = roxmltree::Document::parse(&svg).expect("valid SVG");
        assert_eq!(document.root_element().tag_name().name(), "svg");
    }

    #[test]
    fn labels_are_escaped_and_wrapped() {
        let svg = to_svg(&sample(), &SvgOptions::default());
        assert!(svg.contains("A &amp; B &lt;tag&gt;"));
        assert!(svg.contains("Root &quot;quoted&quot;"));
        assert!(!svg.contains("A & B <tag>"));
        assert!(svg.contains("width="));
        assert!(svg.contains("height="));
    }

    #[test]
    fn theme_override_and_alias_work() {
        let svg = to_svg(
            &sample(),
            &SvgOptions {
                theme: Some("ocean".to_owned()),
                ..SvgOptions::default()
            },
        );
        assert!(svg.contains("#071a23"));
        assert!(!svg.contains("#f7f8fb"));
    }

    #[test]
    fn custom_node_color_and_collapse_indicator_are_rendered() {
        let mut map = sample();
        let child = map.root.children[0].id.clone();
        map.set_collapsed(&child, true).unwrap();
        let svg = to_svg(&map, &SvgOptions::default());
        assert!(svg.contains("#ffcc00"));
        assert!(svg.contains("<circle"));
    }

    #[test]
    fn layout_override_changes_geometry() {
        let map = sample();
        let radial = to_svg(
            &map,
            &SvgOptions {
                layout: Some(LayoutKind::Radial),
                ..SvgOptions::default()
            },
        );
        let logical = to_svg(
            &map,
            &SvgOptions {
                layout: Some(LayoutKind::Logical),
                ..SvgOptions::default()
            },
        );
        assert_ne!(radial, logical);
        assert!(radial.contains("<path"));
    }

    #[test]
    fn empty_map_still_produces_a_drawing() {
        let map = MindMap::new("Empty", "");
        let svg = to_svg(&map, &SvgOptions::default());
        assert!(roxmltree::Document::parse(&svg).is_ok());
        assert!(svg.contains("<rect"));
    }

    #[test]
    fn first_level_branches_get_distinct_hues() {
        let mut map = MindMap::new("Branches", "Root");
        map.theme = "light".to_owned();
        let root = map.root.id.clone();
        map.add_child(&root, "First branch").unwrap();
        map.add_child(&root, "Second branch").unwrap();
        let svg = to_svg(&map, &SvgOptions::default());
        let fills = Palette::by_name("light").branch_fills();
        assert!(svg.contains(fills[0]));
        assert!(svg.contains(fills[1]));
    }
}
