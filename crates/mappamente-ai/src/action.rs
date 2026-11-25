//! JSON action protocol used by the in-app assistant.
//!
//! The model does not receive provider-specific tools. Instead it returns one
//! JSON object containing a short reply and a list of map actions. This keeps
//! OpenAI-compatible and Anthropic providers on the same code path and lets the
//! GUI apply every turn as a single undoable transaction.

use std::collections::HashMap;

use mappamente_core::io::markdown;
use mappamente_core::model::LayoutKind;
use mappamente_core::theme::Palette;
use mappamente_core::{Error, MindMap, Result};
use serde::{Deserialize, Deserializer, Serialize};

use crate::error::{AiError, Result as AiResult};

/// Maximum label/note length, matching the MCP limits.
pub const MAX_TEXT_CHARS: usize = 10_000;

/// Maximum Markdown outline length accepted by an action.
pub const MAX_MARKDOWN_CHARS: usize = 100_000;

/// A single map edit requested by the model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum MapAction {
    /// Adds one node and optionally records a temporary id for later actions.
    AddNode {
        /// Existing id or a temp id created earlier in this plan.
        parent_id: String,
        /// New label.
        text: String,
        /// Child position; omitted appends.
        #[serde(default)]
        index: Option<usize>,
        /// Optional alias usable by later actions in the same plan.
        #[serde(default)]
        temp_id: Option<String>,
        /// Optional note.
        #[serde(default)]
        note: Option<String>,
        /// Optional `#RRGGBB`/`#RRGGBBAA` color.
        #[serde(default)]
        color: Option<String>,
        /// Optional collapsed state.
        #[serde(default)]
        collapsed: Option<bool>,
    },

    /// Adds a nested subtree in one action.
    AddTree {
        /// Existing id or a temp id created earlier in this plan.
        parent_id: String,
        /// Root and descendants of the new subtree.
        node: ActionNode,
        /// Child position; omitted appends.
        #[serde(default)]
        index: Option<usize>,
        /// Optional alias for the new subtree root.
        #[serde(default)]
        temp_id: Option<String>,
    },

    /// Inserts a Markdown outline below a node.
    AddSubtreeFromMarkdown {
        /// Existing id or a temp id created earlier in this plan.
        parent_id: String,
        /// Single-root Markdown outline.
        markdown: String,
        /// Child position; omitted appends.
        #[serde(default)]
        index: Option<usize>,
    },

    /// Changes one or more fields of an existing or newly added node.
    UpdateNode {
        /// Existing id or a temp id created earlier in this plan.
        node_id: String,
        /// New label.
        #[serde(default)]
        text: Option<String>,
        /// New note.
        #[serde(default)]
        note: Option<String>,
        /// New color; an empty string clears it.
        #[serde(default)]
        color: Option<String>,
        /// New collapsed state.
        #[serde(default)]
        collapsed: Option<bool>,
    },

    /// Reparents a node.
    MoveNode {
        /// Node to move.
        node_id: String,
        /// New parent.
        new_parent_id: String,
        /// Position below the new parent; omitted appends.
        #[serde(default)]
        index: Option<usize>,
    },

    /// Removes a node and its subtree.
    DeleteNode {
        /// Node to remove.
        node_id: String,
    },

    /// Changes the layout algorithm.
    SetLayout {
        /// `radial`, `logical`, `tree`, or `bilateral`.
        layout: String,
    },

    /// Changes the document title.
    SetTitle {
        /// New title (maximum 10,000 characters).
        title: String,
    },

    /// Changes the theme.
    SetTheme {
        /// `dark`, `light`, `paper`, or `ocean`.
        theme: String,
    },
}

/// A node in an [`MapAction::AddTree`] action.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionNode {
    /// Label.
    pub text: String,
    /// Optional note.
    #[serde(default)]
    pub note: Option<String>,
    /// Optional color.
    #[serde(default)]
    pub color: Option<String>,
    /// Optional collapsed state.
    #[serde(default)]
    pub collapsed: Option<bool>,
    /// Child nodes, in display order.
    #[serde(default)]
    pub children: Vec<ActionNode>,
}

/// One assistant turn: a reply for the user and optional map edits.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    /// Short response shown in the chat.
    #[serde(default, deserialize_with = "null_as_default")]
    pub reply: String,
    /// Edits to apply in order.
    #[serde(default, deserialize_with = "null_as_default")]
    pub actions: Vec<MapAction>,
}

/// Counts and ids produced by applying a plan.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ApplySummary {
    /// Number of actions applied.
    pub applied: usize,
    /// Ids created, changed or removed, in execution order.
    pub node_ids: Vec<String>,
}

/// Parses a model response into a [`Plan`].
///
/// Accepts plain JSON, a fenced code block, JSON surrounded by prose, and a
/// bare array of actions (treated as a plan with an empty reply).
pub fn parse_plan(raw: &str) -> AiResult<Plan> {
    let text = strip_code_fence(raw);
    let mut candidates = vec![text];
    if let Some((start, end)) = json_slice(text)
        && (start != 0 || end != text.len())
    {
        candidates.push(&text[start..end]);
    }

    let mut last_error = None;
    for candidate in candidates {
        match serde_json::from_str::<Plan>(candidate) {
            Ok(plan) => return Ok(plan),
            Err(plan_error) => match serde_json::from_str::<Vec<MapAction>>(candidate) {
                Ok(actions) => {
                    return Ok(Plan {
                        reply: String::new(),
                        actions,
                    });
                }
                Err(_) => last_error = Some(plan_error),
            },
        }
    }

    let error = last_error
        .map(|error| error.to_string())
        .unwrap_or_else(|| "empty response".to_owned());
    Err(AiError::InvalidResponse(format!(
        "{error} (response began with {:?})",
        excerpt(text)
    )))
}

/// Applies actions in order to `map`.
///
/// The caller should run this inside [`mappamente_core::MindMap::edit`] or
/// [`mappamente_core::Editor::apply`] so a failure rolls the whole plan back.
pub fn apply_actions(map: &mut MindMap, actions: &[MapAction]) -> Result<ApplySummary> {
    let mut summary = ApplySummary::default();
    let mut temps = HashMap::new();
    for (index, action) in actions.iter().enumerate() {
        apply_one(map, action, &mut temps, &mut summary)
            .map_err(|error| Error::Parse(format!("AI action {}: {error}", index + 1)))?;
    }
    Ok(summary)
}

fn apply_one(
    map: &mut MindMap,
    action: &MapAction,
    temps: &mut HashMap<String, String>,
    summary: &mut ApplySummary,
) -> Result<()> {
    match action {
        MapAction::AddNode {
            parent_id,
            text,
            index,
            temp_id,
            note,
            color,
            collapsed,
        } => {
            check_text("text", text)?;
            if let Some(note) = note {
                check_text("note", note)?;
            }
            let parent = resolve(temps, parent_id);
            let id = map.add_child_at(&parent, index.unwrap_or(usize::MAX), text.clone())?;
            apply_fields(map, &id, note.as_deref(), color.as_deref(), *collapsed)?;
            remember_temp(map, temps, temp_id.as_deref(), &id)?;
            summary.node_ids.push(id);
        }
        MapAction::AddTree {
            parent_id,
            node,
            index,
            temp_id,
        } => {
            let parent = resolve(temps, parent_id);
            let id = add_tree(map, &parent, index.unwrap_or(usize::MAX), node)?;
            remember_temp(map, temps, temp_id.as_deref(), &id)?;
            summary.node_ids.push(id);
        }
        MapAction::AddSubtreeFromMarkdown {
            parent_id,
            markdown: outline,
            index,
        } => {
            if outline.chars().count() > MAX_MARKDOWN_CHARS {
                return Err(Error::Parse(format!(
                    "markdown is too long ({} characters; maximum {MAX_MARKDOWN_CHARS})",
                    outline.chars().count()
                )));
            }
            let parent = resolve(temps, parent_id);
            let ids = markdown::insert_markdown_children(
                map,
                &parent,
                index.unwrap_or(usize::MAX),
                outline,
            )?;
            summary.node_ids.extend(ids);
        }
        MapAction::UpdateNode {
            node_id,
            text,
            note,
            color,
            collapsed,
        } => {
            if text.is_none() && note.is_none() && color.is_none() && collapsed.is_none() {
                return Err(Error::Parse("update_node has no fields".to_owned()));
            }
            if let Some(text) = text {
                check_text("text", text)?;
            }
            if let Some(note) = note {
                check_text("note", note)?;
            }
            let id = resolve(temps, node_id);
            apply_fields(map, &id, note.as_deref(), color.as_deref(), *collapsed)?;
            if let Some(text) = text {
                map.set_text(&id, text.clone())?;
            }
            summary.node_ids.push(id);
        }
        MapAction::MoveNode {
            node_id,
            new_parent_id,
            index,
        } => {
            let id = resolve(temps, node_id);
            let parent = resolve(temps, new_parent_id);
            map.move_node(&id, &parent, index.unwrap_or(usize::MAX))?;
            summary.node_ids.push(id);
        }
        MapAction::DeleteNode { node_id } => {
            let id = resolve(temps, node_id);
            let removed = map.remove(&id)?;
            summary.node_ids.push(removed.id);
        }
        MapAction::SetLayout { layout } => {
            map.set_layout(parse_layout(layout)?);
        }
        MapAction::SetTitle { title } => {
            check_text("title", title)?;
            map.title = title.clone();
        }
        MapAction::SetTheme { theme } => {
            let canonical = Palette::NAMES
                .iter()
                .find(|name| name.eq_ignore_ascii_case(theme))
                .ok_or_else(|| {
                    Error::Parse(format!(
                        "unknown theme {theme:?}; expected dark, light, paper or ocean"
                    ))
                })?;
            map.theme = (*canonical).to_owned();
        }
    }
    summary.applied += 1;
    Ok(())
}

fn add_tree(map: &mut MindMap, parent: &str, index: usize, node: &ActionNode) -> Result<String> {
    check_text("text", &node.text)?;
    if let Some(note) = &node.note {
        check_text("note", note)?;
    }
    let id = map.add_child_at(parent, index, node.text.clone())?;
    apply_fields(
        map,
        &id,
        node.note.as_deref(),
        node.color.as_deref(),
        node.collapsed,
    )?;
    for child in &node.children {
        add_tree(map, &id, usize::MAX, child)?;
    }
    Ok(id)
}

fn apply_fields(
    map: &mut MindMap,
    id: &str,
    note: Option<&str>,
    color: Option<&str>,
    collapsed: Option<bool>,
) -> Result<()> {
    if let Some(note) = note {
        map.set_note(id, note)?;
    }
    if let Some(color) = color {
        let color = if color.is_empty() { None } else { Some(color) };
        map.set_color(id, color)?;
    }
    if let Some(collapsed) = collapsed {
        map.set_collapsed(id, collapsed)?;
    }
    Ok(())
}

fn remember_temp(
    map: &MindMap,
    temps: &mut HashMap<String, String>,
    temp_id: Option<&str>,
    real_id: &str,
) -> Result<()> {
    let Some(temp_id) = temp_id else {
        return Ok(());
    };
    let temp_id = temp_id.trim();
    if temp_id.is_empty() {
        return Err(Error::Parse("temp_id cannot be empty".to_owned()));
    }
    if map.find(temp_id).is_some() {
        return Err(Error::Parse(format!(
            "temp_id {temp_id:?} collides with an existing node"
        )));
    }
    if temps
        .insert(temp_id.to_owned(), real_id.to_owned())
        .is_some()
    {
        return Err(Error::Parse(format!("duplicate temp_id {temp_id:?}")));
    }
    Ok(())
}

fn resolve(temps: &HashMap<String, String>, id: &str) -> String {
    temps.get(id).cloned().unwrap_or_else(|| id.to_owned())
}

fn check_text(field: &str, text: &str) -> Result<()> {
    let chars = text.chars().count();
    if chars > MAX_TEXT_CHARS {
        return Err(Error::Parse(format!(
            "{field} is too long ({chars} characters; maximum {MAX_TEXT_CHARS})"
        )));
    }
    Ok(())
}

fn parse_layout(value: &str) -> Result<LayoutKind> {
    match value.to_ascii_lowercase().as_str() {
        "tree" | "bilateral" => Ok(LayoutKind::Tree),
        "radial" => Ok(LayoutKind::Radial),
        "logical" => Ok(LayoutKind::Logical),
        other => Err(Error::Parse(format!(
            "unknown layout {other:?}; expected radial, logical or tree"
        ))),
    }
}

fn strip_code_fence(raw: &str) -> &str {
    let text = raw.trim_start_matches('\u{feff}').trim();
    let Some(rest) = text.strip_prefix("```") else {
        return text;
    };
    let rest = rest.trim_start_matches(['\r', '\n']);
    let rest = match rest.find(['\n', '\r']) {
        Some(index)
            if !rest[..index].trim_start().starts_with('{')
                && !rest[..index].trim_start().starts_with('[') =>
        {
            &rest[index + 1..]
        }
        _ => rest,
    };
    rest.strip_suffix("```").unwrap_or(rest).trim()
}

fn json_slice(text: &str) -> Option<(usize, usize)> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    (end > start).then_some((start, end + 1))
}

fn excerpt(text: &str) -> String {
    text.chars().take(120).collect()
}

fn null_as_default<'de, D, T>(deserializer: D) -> std::result::Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root_id(map: &MindMap) -> String {
        map.root.id.clone()
    }

    #[test]
    fn parses_plain_fenced_and_wrapped_json() {
        let raw = r#"{"reply":"ok","actions":[]}"#;
        assert_eq!(parse_plan(raw).unwrap().reply, "ok");

        let fenced = format!("```json\n{raw}\n```");
        assert_eq!(parse_plan(&fenced).unwrap().reply, "ok");

        let fenced_without_language = format!("```\n{raw}\n```");
        assert_eq!(parse_plan(&fenced_without_language).unwrap().reply, "ok");

        let wrapped = format!("Here is the plan:\n{raw}\nDone.");
        assert_eq!(parse_plan(&wrapped).unwrap().reply, "ok");
    }

    #[test]
    fn parses_a_bare_action_array() {
        let raw = r#"[{"op":"set_title","title":"New"}]"#;
        let plan = parse_plan(raw).unwrap();
        assert!(plan.reply.is_empty());
        assert_eq!(plan.actions.len(), 1);
    }

    #[test]
    fn accepts_null_reply_and_actions() {
        let plan = parse_plan(r#"{"reply":null,"actions":null}"#).unwrap();
        assert!(plan.reply.is_empty());
        assert!(plan.actions.is_empty());
    }

    #[test]
    fn rejects_invalid_json() {
        let error = parse_plan("not json").unwrap_err();
        assert!(matches!(error, AiError::InvalidResponse(_)));
    }

    #[test]
    fn applies_nested_trees_and_temp_ids() {
        let mut map = MindMap::new("Test", "Root");
        let root = root_id(&map);
        let plan = parse_plan(&format!(
            r#"{{"reply":"ok","actions":[
                {{"op":"add_node","parent_id":"{root}","text":"Branch","temp_id":"new1"}},
                {{"op":"add_node","parent_id":"new1","text":"Child"}},
                {{"op":"add_tree","parent_id":"new1","node":{{"text":"Tree","children":[{{"text":"Leaf"}}]}}}}
            ]}}"#
        ))
        .unwrap();

        let summary = map
            .edit(|draft| apply_actions(draft, &plan.actions))
            .unwrap();
        assert_eq!(summary.applied, 3);
        assert_eq!(summary.node_ids.len(), 3);
        let branch = map.find(&summary.node_ids[0]).unwrap();
        assert_eq!(branch.text, "Branch");
        assert_eq!(branch.children.len(), 2);
        assert_eq!(branch.children[0].text, "Child");
        assert_eq!(branch.children[1].text, "Tree");
        assert_eq!(branch.children[1].children[0].text, "Leaf");
        assert!(map.validate().is_ok());
    }

    #[test]
    fn updates_moves_and_deletes_nodes() {
        let mut map = MindMap::new("Test", "Root");
        let root = root_id(&map);
        let a = map.add_child(&root, "A").unwrap();
        let b = map.add_child(&root, "B").unwrap();
        let plan = parse_plan(&format!(
            r##"{{"reply":"ok","actions":[
                {{"op":"update_node","node_id":"{a}","text":"A2","note":"note","color":"#ff8800","collapsed":true}},
                {{"op":"move_node","node_id":"{b}","new_parent_id":"{a}","index":0}},
                {{"op":"delete_node","node_id":"{b}"}}
            ]}}"##
        ))
        .unwrap();

        map.edit(|draft| apply_actions(draft, &plan.actions))
            .unwrap();
        let a_node = map.find(&a).unwrap();
        assert_eq!(a_node.text, "A2");
        assert_eq!(a_node.note, "note");
        assert_eq!(a_node.color.as_deref(), Some("#ff8800"));
        assert!(a_node.collapsed);
        assert!(map.find(&b).is_none());
        assert_eq!(map.node_count(), 2);
    }

    #[test]
    fn changes_document_properties() {
        let mut map = MindMap::new("Test", "Root");
        let plan = parse_plan(
            r#"{"reply":"ok","actions":[
                {"op":"set_layout","layout":"tree"},
                {"op":"set_title","title":"Renamed"},
                {"op":"set_theme","theme":"OCEAN"}
            ]}"#,
        )
        .unwrap();
        map.edit(|draft| apply_actions(draft, &plan.actions))
            .unwrap();
        assert_eq!(map.layout, LayoutKind::Tree);
        assert_eq!(map.title, "Renamed");
        assert_eq!(map.theme, "ocean");
    }

    #[test]
    fn invalid_actions_roll_the_plan_back() {
        let mut map = MindMap::new("Test", "Root");
        let root = root_id(&map);
        let before = map.clone();
        let plan = parse_plan(&format!(
            r#"{{"reply":"ok","actions":[
                {{"op":"add_node","parent_id":"{root}","text":"Temporary"}},
                {{"op":"delete_node","node_id":"{root}"}}
            ]}}"#
        ))
        .unwrap();

        let result = map.edit(|draft| apply_actions(draft, &plan.actions));
        assert!(result.is_err());
        assert_eq!(map, before);
    }

    #[test]
    fn invalid_color_rolls_back() {
        let mut map = MindMap::new("Test", "Root");
        let root = root_id(&map);
        let before = map.clone();
        let plan = parse_plan(&format!(
            r#"{{"reply":"ok","actions":[
                {{"op":"update_node","node_id":"{root}","color":"not-a-color"}}
            ]}}"#
        ))
        .unwrap();
        assert!(
            map.edit(|draft| apply_actions(draft, &plan.actions))
                .is_err()
        );
        assert_eq!(map, before);
    }

    #[test]
    fn limits_are_enforced() {
        let mut map = MindMap::new("Test", "Root");
        let root = root_id(&map);
        let text = "x".repeat(MAX_TEXT_CHARS + 1);
        let plan = parse_plan(&format!(
            r#"{{"reply":"ok","actions":[{{"op":"add_node","parent_id":"{root}","text":"{text}"}}]}}"#
        ))
        .unwrap();
        assert!(
            map.edit(|draft| apply_actions(draft, &plan.actions))
                .is_err()
        );
        assert_eq!(map.node_count(), 1);
    }
}
