//! Editing canvas for `.mappa` documents.
//!
//! Double-click a node to edit its label, `Tab` adds a child, `Enter` adds a
//! sibling, `Delete` removes a branch, drag a node to move or reparent it, and
//! drag the background to pan. All changes go through [`Editor`], so undo,
//! redo, coalesced drags and dirty tracking are shared with the core.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use anyhow::Result;
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Key, Modifiers, RichText, Sense, Stroke,
    StrokeKind,
};
use mappamente_ai::apply_actions;
use mappamente_core::Editor;
use mappamente_core::io;
use mappamente_core::layout::{Layout, LayoutNode, LayoutOptions, Metrics};
use mappamente_core::model::{LayoutKind, MindMap, Node, NodeId};
use mappamente_core::theme::{Palette, parse_hex};

use crate::ai_panel::{AiPanel, AiPanelAction};
use crate::icons::{self, Icon};
use crate::style::{self, Tokens};
use crate::ui_prefs::UiPrefs;

const APP_NAME: &str = "Mappamente";

/// Opens the editor, optionally loading `initial`.
pub fn run(initial: Option<PathBuf>) -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 600.0])
            .with_min_inner_size([720.0, 480.0])
            .with_title(APP_NAME)
            .with_icon(window_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "mappamente",
        options,
        Box::new(move |creation| {
            let system_theme = creation.egui_ctx.system_theme().map(|theme| {
                match theme {
                    egui::Theme::Dark => "dark",
                    egui::Theme::Light => "light",
                }
                .to_owned()
            });
            let mut app = MappamenteApp::new(initial.clone(), system_theme);
            app.apply_visuals(&creation.egui_ctx);
            Ok(Box::new(app))
        }),
    )
    .map_err(|error| anyhow::anyhow!("cannot open the Mappamente window: {error}"))
}

struct EditingState {
    id: NodeId,
    buffer: String,
    is_new: bool,
    focus: bool,
}

struct DragState {
    id: NodeId,
    /// Offset from the node's top-left to the pointer, in world coordinates.
    grab: mappamente_core::Point,
    start_screen: egui::Pos2,
    moved: bool,
}

struct FitAnimation {
    start_time: f64,
    from_zoom: f32,
    to_zoom: f32,
    from_center: mappamente_core::Point,
    to_center: mappamente_core::Point,
}

struct MappamenteApp {
    editor: Editor,
    path: Option<PathBuf>,
    kind: LayoutKind,
    theme: String,
    tokens: Tokens,
    applied_style: Option<(String, bool)>,
    layout: Layout,
    colors: HashMap<String, Color32>,
    branch_of: HashMap<String, usize>,
    zoom: f32,
    view_center: mappamente_core::Point,
    fit_pending: bool,
    fit_animation: Option<FitAnimation>,
    error: Option<String>,
    selected: Option<NodeId>,
    hovered: Option<NodeId>,
    editing: Option<EditingState>,
    drag: Option<DragState>,
    window_title: String,
    allow_close: bool,
    ai: AiPanel,
    ai_open: bool,
    ai_width: f32,
    prefs: UiPrefs,
    system_theme: Option<String>,
    known_nodes: HashSet<String>,
    node_birth: HashMap<String, f64>,
    nodes_initialized: bool,
}

impl MappamenteApp {
    fn new(initial: Option<PathBuf>, system_theme: Option<String>) -> Self {
        let prefs = UiPrefs::load();
        let default_theme = prefs.theme_or(system_theme.as_deref());
        let mut error = None;
        let (map, path) = match initial {
            Some(path) => match io::load_path(&path) {
                Ok(map) => (map, Some(path)),
                Err(load_error) => {
                    error = Some(format!("Could not open {}: {load_error}", path.display()));
                    (welcome_map(&default_theme), None)
                }
            },
            None => (welcome_map(&default_theme), None),
        };

        let selected = Some(map.root.id.clone());
        let kind = map.layout;
        let theme = Palette::by_name(&map.theme).name.to_owned();
        let tokens = Tokens::for_theme(&theme);
        let layout = compute_layout(&map, kind);
        let colors = collect_colors(&map);
        let branch_of = collect_branches(&map);
        let editor = Editor::new(map);

        Self {
            editor,
            path,
            kind,
            theme,
            tokens,
            applied_style: None,
            layout,
            colors,
            branch_of,
            zoom: 1.0,
            view_center: mappamente_core::Point::ZERO,
            fit_pending: true,
            fit_animation: None,
            error,
            selected,
            hovered: None,
            editing: None,
            drag: None,
            window_title: String::new(),
            allow_close: false,
            ai: AiPanel::new(),
            ai_open: false,
            ai_width: 380.0,
            prefs,
            system_theme,
            known_nodes: HashSet::new(),
            node_birth: HashMap::new(),
            nodes_initialized: false,
        }
    }

    fn map(&self) -> &MindMap {
        self.editor.map()
    }

    fn load(&mut self, path: PathBuf) {
        self.commit_editing();
        if !self.confirm_discard() {
            return;
        }
        match io::load_path(&path) {
            Ok(map) => {
                self.kind = map.layout;
                self.theme = Palette::by_name(&map.theme).name.to_owned();
                self.tokens = Tokens::for_theme(&self.theme);
                self.applied_style = None;
                self.layout = compute_layout(&map, self.kind);
                self.colors = collect_colors(&map);
                self.branch_of = collect_branches(&map);
                self.selected = Some(map.root.id.clone());
                self.editor = Editor::new(map);
                self.path = Some(path);
                self.error = None;
                self.editing = None;
                self.drag = None;
                self.hovered = None;
                self.fit_animation = None;
                self.known_nodes.clear();
                self.node_birth.clear();
                self.nodes_initialized = false;
                self.fit_pending = true;
            }
            Err(error) => {
                self.error = Some(format!("Could not open {}: {error}", path.display()));
            }
        }
    }

    fn pick_file(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Open a map")
            .add_filter("Mappamente maps", &["mappa", "json"])
            .add_filter("Markdown outlines", &["md", "markdown"])
            .pick_file()
        {
            self.load(path);
        }
    }

    fn new_map(&mut self) {
        self.commit_editing();
        if !self.confirm_discard() {
            return;
        }
        let theme = self.prefs.theme_or(self.system_theme.as_deref());
        let mut map = MindMap::new("Untitled map", "Central idea");
        map.theme = theme;
        let root = map.root.id.clone();
        self.kind = map.layout;
        self.editor = Editor::new(map);
        self.editor.mark_dirty();
        self.path = None;
        self.selected = Some(root);
        self.error = None;
        self.editing = None;
        self.drag = None;
        self.hovered = None;
        self.fit_animation = None;
        self.applied_style = None;
        self.nodes_initialized = false;
        self.known_nodes.clear();
        self.node_birth.clear();
        self.after_change();
        self.fit_pending = true;
    }

    fn save(&mut self) {
        self.commit_editing();
        let Some(path) = self.path.clone() else {
            self.save_as();
            return;
        };
        if matches!(io::extension(&path).as_deref(), Some("mappa" | "json")) {
            self.save_to(path);
        } else {
            // Markdown and other imports are lossy: force a native save.
            self.save_as();
        }
    }

    fn save_as(&mut self) {
        self.commit_editing();
        let title = sanitize_file_name(&self.map().title);
        let Some(mut path) = rfd::FileDialog::new()
            .set_title("Save map")
            .set_file_name(format!("{title}.mappa"))
            .add_filter("Mappamente map", &["mappa"])
            .save_file()
        else {
            return;
        };
        if path.extension().is_none() {
            path.set_extension("mappa");
        }
        if io::extension(&path).as_deref() != Some("mappa") {
            self.error = Some("Save the native format with the .mappa extension".to_owned());
            return;
        }
        self.save_to(path);
    }

    fn save_to(&mut self, path: PathBuf) {
        match io::save_path(&path, self.map()) {
            Ok(()) => {
                self.editor.mark_saved();
                self.path = Some(path);
                self.error = None;
            }
            Err(error) => {
                self.error = Some(format!("Could not save {}: {error}", path.display()));
            }
        }
    }

    fn confirm_discard(&self) -> bool {
        if !self.editor.is_dirty() {
            return true;
        }
        rfd::MessageDialog::new()
            .set_title("Unsaved changes")
            .set_description("Discard the unsaved changes?")
            .set_level(rfd::MessageLevel::Warning)
            .set_buttons(rfd::MessageButtons::YesNo)
            .show()
            == rfd::MessageDialogResult::Yes
    }

    /// Recomputes derived data after a document change and prunes stale ids.
    fn after_change(&mut self) {
        self.kind = self.editor.map().layout;
        let theme = Palette::by_name(&self.editor.map().theme).name.to_owned();
        if theme != self.theme {
            self.theme = theme;
            self.tokens = Tokens::for_theme(&self.theme);
            self.applied_style = None;
        }

        let selected = self.selected.clone();
        let hovered = self.hovered.clone();
        let editing = self.editing.as_ref().map(|state| state.id.clone());
        let drag = self.drag.as_ref().map(|state| state.id.clone());

        let (colors, branches, layout, selected_valid, hovered_valid, editing_valid, drag_valid) = {
            let map = self.map();
            (
                collect_colors(map),
                collect_branches(map),
                compute_layout(map, self.kind),
                selected.as_deref().is_some_and(|id| map.find(id).is_some()),
                hovered.as_deref().is_some_and(|id| map.find(id).is_some()),
                editing.as_deref().is_some_and(|id| map.find(id).is_some()),
                drag.as_deref().is_some_and(|id| map.find(id).is_some()),
            )
        };

        self.colors = colors;
        self.branch_of = branches;
        self.layout = layout;
        if selected.is_some() && !selected_valid {
            self.selected = None;
        }
        if hovered.is_some() && !hovered_valid {
            self.hovered = None;
        }
        if editing.is_some() && !editing_valid {
            self.editing = None;
        }
        if drag.is_some() && !drag_valid {
            self.drag = None;
        }
    }

    fn apply<F, T>(&mut self, label: &str, change: F) -> Option<T>
    where
        F: FnOnce(&mut MindMap) -> mappamente_core::Result<T>,
    {
        match self.editor.apply(label, change) {
            Ok(value) => {
                self.error = None;
                self.after_change();
                Some(value)
            }
            Err(error) => {
                self.error = Some(error.to_string());
                None
            }
        }
    }

    fn apply_coalesced<F>(&mut self, label: &str, now_ms: u64, window_ms: u64, change: F) -> bool
    where
        F: FnOnce(&mut MindMap) -> mappamente_core::Result<()>,
    {
        match self
            .editor
            .apply_coalesced(label, now_ms, window_ms, change)
        {
            Ok(()) => {
                self.error = None;
                self.after_change();
                true
            }
            Err(error) => {
                self.error = Some(error.to_string());
                false
            }
        }
    }

    fn undo(&mut self) {
        if self.editor.undo() {
            self.editing = None;
            self.drag = None;
            self.error = None;
            self.after_change();
        }
    }

    fn redo(&mut self) {
        if self.editor.redo() {
            self.editing = None;
            self.drag = None;
            self.error = None;
            self.after_change();
        }
    }

    fn handle_ai_action(&mut self, action: AiPanelAction) {
        match action {
            AiPanelAction::Apply { reply, plan } => {
                self.commit_editing();
                if plan.actions.is_empty() {
                    self.ai.finish(reply, 0);
                    return;
                }
                match self
                    .editor
                    .apply("AI edit", |map| apply_actions(map, &plan.actions))
                {
                    Ok(summary) => {
                        self.error = None;
                        self.drag = None;
                        if let Some(id) = summary.node_ids.first() {
                            self.selected = Some(id.clone());
                        }
                        self.after_change();
                        self.ai.finish(reply, summary.applied);
                    }
                    Err(error) => {
                        let message = format!("L'AI ha proposto modifiche non valide: {error}");
                        self.error = Some(message.clone());
                        self.ai.fail(reply, message);
                    }
                }
            }
        }
    }

    fn start_editing(&mut self, id: NodeId, is_new: bool) {
        self.commit_editing();
        let Some(text) = self.map().find(&id).map(|node| node.text.clone()) else {
            return;
        };
        self.selected = Some(id.clone());
        self.editing = Some(EditingState {
            id,
            buffer: text,
            is_new,
            focus: true,
        });
        self.drag = None;
    }

    fn edit_selected(&mut self) {
        if let Some(id) = self.selected.clone() {
            self.start_editing(id, false);
        }
    }

    fn commit_editing(&mut self) {
        let Some(edit) = self.editing.take() else {
            return;
        };
        let Some(current) = self.map().find(&edit.id).map(|node| node.text.clone()) else {
            return;
        };

        let mut text = edit.buffer.trim().to_owned();
        if edit.is_new && text.is_empty() {
            let parent = self.map().parent_of(&edit.id).map(|node| node.id.clone());
            self.apply("discard empty node", |map| map.remove(&edit.id).map(|_| ()));
            self.selected = parent;
            return;
        }
        if text.is_empty() {
            text = "Untitled".to_owned();
        }
        if text == current {
            return;
        }
        self.apply("edit text", |map| map.set_text(&edit.id, text));
    }

    fn cancel_editing(&mut self) {
        let Some(edit) = self.editing.take() else {
            return;
        };
        if edit.is_new {
            let parent = self.map().parent_of(&edit.id).map(|node| node.id.clone());
            self.apply("discard empty node", |map| map.remove(&edit.id).map(|_| ()));
            self.selected = parent;
        }
    }

    fn add_child_to(&mut self, parent: NodeId) {
        self.add_node_at(parent, usize::MAX);
    }

    fn add_node_at(&mut self, parent: NodeId, index: usize) {
        if let Some(id) = self.apply("add node", |map| map.add_child_at(&parent, index, "")) {
            self.selected = Some(id.clone());
            self.start_editing(id, true);
        }
    }

    fn add_child(&mut self) {
        let root = self.map().root.id.clone();
        let parent = self
            .selected
            .clone()
            .filter(|id| self.map().find(id).is_some())
            .unwrap_or(root);
        self.add_child_to(parent);
    }

    fn add_sibling(&mut self) {
        let root = self.map().root.id.clone();
        let selected = self.selected.clone().unwrap_or_else(|| root.clone());
        let (parent, index) = if selected == root {
            (root, usize::MAX)
        } else {
            match self.map().parent_of(&selected) {
                Some(parent) => {
                    let index = parent
                        .children
                        .iter()
                        .position(|node| node.id == selected)
                        .map_or(usize::MAX, |position| position + 1);
                    (parent.id.clone(), index)
                }
                None => (self.map().root.id.clone(), usize::MAX),
            }
        };
        self.add_node_at(parent, index);
    }

    fn delete_selected(&mut self) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        if id == self.map().root.id {
            self.error = Some("The root node cannot be deleted".to_owned());
            return;
        }
        let parent = self.map().parent_of(&id).map(|node| node.id.clone());
        if self
            .apply("delete node", |map| map.remove(&id).map(|_| ()))
            .is_some()
        {
            self.selected = parent;
        }
    }

    fn navigate(&mut self, key: Key) {
        let root = self.map().root.id.clone();
        let Some(selected) = self.selected.clone() else {
            self.selected = Some(root);
            return;
        };
        let target = match key {
            Key::ArrowLeft => self.map().parent_of(&selected).map(|node| node.id.clone()),
            Key::ArrowRight => self
                .map()
                .find(&selected)
                .and_then(|node| node.children.first())
                .map(|node| node.id.clone()),
            Key::ArrowUp | Key::ArrowDown => {
                if selected == root {
                    None
                } else {
                    self.map().parent_of(&selected).and_then(|parent| {
                        let position = parent
                            .children
                            .iter()
                            .position(|node| node.id == selected)?;
                        let index = if key == Key::ArrowUp {
                            position.checked_sub(1)
                        } else {
                            Some(position + 1)
                        };
                        index
                            .and_then(|index| parent.children.get(index))
                            .map(|node| node.id.clone())
                    })
                }
            }
            _ => None,
        };
        if let Some(target) = target {
            self.selected = Some(target);
        }
    }

    fn handle_keys(&mut self, context: &egui::Context) {
        if self.editing.is_some() || context.egui_wants_keyboard_input() {
            return;
        }

        let command_shift = Modifiers {
            command: true,
            shift: true,
            ..Default::default()
        };
        if context.input_mut(|input| input.consume_key(command_shift, Key::A)) {
            if !self.ai_open {
                self.commit_editing();
            }
            self.ai_open = !self.ai_open;
        }
        if context.input_mut(|input| input.consume_key(command_shift, Key::Z)) {
            self.redo();
        } else if context.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::Z)) {
            self.undo();
        }
        if context.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::Y)) {
            self.redo();
        }
        if context.input_mut(|input| input.consume_key(command_shift, Key::S)) {
            self.save_as();
        } else if context.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::S)) {
            self.save();
        }
        if context.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::O)) {
            self.pick_file();
        }
        if context.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::N)) {
            self.new_map();
        }
        if context.input_mut(|input| input.consume_key(Modifiers::NONE, Key::F)) {
            self.fit_pending = true;
        }
        if context.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Tab)) {
            self.add_child();
        }
        if context.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter)) {
            self.add_sibling();
        }
        if context.input_mut(|input| input.consume_key(Modifiers::NONE, Key::F2)) {
            self.edit_selected();
        }
        if context.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Delete))
            || context.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Backspace))
        {
            self.delete_selected();
        }
        if context.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape)) {
            self.selected = None;
        }
        for key in [
            Key::ArrowLeft,
            Key::ArrowRight,
            Key::ArrowUp,
            Key::ArrowDown,
        ] {
            if context.input_mut(|input| input.consume_key(Modifiers::NONE, key)) {
                self.navigate(key);
            }
        }
    }

    fn handle_close(&mut self, context: &egui::Context) {
        if self.allow_close || !context.input(|input| input.viewport().close_requested()) {
            return;
        }
        self.commit_editing();
        if self.editor.is_dirty() && !self.confirm_discard() {
            context.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        } else {
            self.allow_close = true;
        }
    }

    fn update_window_title(&mut self, context: &egui::Context) {
        let file = self
            .path
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| self.map().title.clone());
        let dirty = if self.editor.is_dirty() { " *" } else { "" };
        let title = format!("{APP_NAME} — {file}{dirty}");
        if title != self.window_title {
            context.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }

    fn apply_visuals(&mut self, context: &egui::Context) {
        let key = (self.theme.clone(), self.prefs.animations);
        if self.applied_style.as_ref() == Some(&key) {
            return;
        }
        style::apply(context, &self.tokens, self.prefs.animations);
        self.applied_style = Some(key);
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let tokens = self.tokens;
        let can_undo = self.editing.is_none() && self.editor.can_undo();
        let can_redo = self.editing.is_none() && self.editor.can_redo();

        egui::Area::new(egui::Id::unique("mappamente_toolbar"))
            .anchor(
                Align2::CENTER_TOP,
                egui::vec2(
                    if self.ai_open {
                        -self.ai_width * 0.5
                    } else {
                        0.0
                    },
                    12.0,
                ),
            )
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::default()
                    .fill(tokens.surface)
                    .stroke(Stroke::new(1.0, tokens.border))
                    .corner_radius(CornerRadius::same(style::RADIUS_PANEL))
                    .shadow(tokens.floating_shadow())
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if self.toolbar_button(ui, Icon::New, false, true, "New map · Ctrl+N")
                            {
                                self.new_map();
                            }
                            if self.toolbar_button(ui, Icon::Open, false, true, "Open map · Ctrl+O")
                            {
                                self.pick_file();
                            }
                            if self.toolbar_button(ui, Icon::Save, false, true, "Save · Ctrl+S") {
                                self.save();
                            }
                            if !self.ai_open
                                && self.toolbar_button(
                                    ui,
                                    Icon::SaveAs,
                                    false,
                                    true,
                                    "Save as · Ctrl+Shift+S",
                                )
                            {
                                self.save_as();
                            }

                            ui.separator();
                            if self.toolbar_button(ui, Icon::Undo, false, can_undo, "Undo · Ctrl+Z")
                            {
                                self.undo();
                            }
                            if self.toolbar_button(
                                ui,
                                Icon::Redo,
                                false,
                                can_redo,
                                "Redo · Ctrl+Shift+Z / Ctrl+Y",
                            ) {
                                self.redo();
                            }
                            if self.toolbar_button(ui, Icon::Fit, false, true, "Fit map · F") {
                                self.fit_pending = true;
                            }

                            ui.separator();
                            if self.toolbar_button(
                                ui,
                                Icon::Ai,
                                self.ai_open,
                                true,
                                "AI assistant · Ctrl+Shift+A",
                            ) {
                                if !self.ai_open {
                                    self.commit_editing();
                                }
                                self.ai_open = !self.ai_open;
                            }

                            ui.separator();
                            if !self.ai_open {
                                let layout_tip =
                                    format!("Layout: {} · click to cycle", layout_name(self.kind));
                                if self.toolbar_button(ui, Icon::Layout, false, true, &layout_tip) {
                                    self.cycle_layout();
                                }
                                let theme_tip = format!("Theme: {} · click to cycle", self.theme);
                                if self.toolbar_button(ui, Icon::Theme, false, true, &theme_tip) {
                                    self.cycle_theme();
                                }
                                if self.toolbar_button(
                                    ui,
                                    Icon::Grid,
                                    self.prefs.grid,
                                    true,
                                    "Dot grid",
                                ) {
                                    self.prefs.grid = !self.prefs.grid;
                                    self.prefs.save();
                                }
                            }

                            ui.menu_button(
                                RichText::new("⋯").size(16.0).color(tokens.text_muted),
                                |ui| {
                                    if ui.button("Save as…").clicked() {
                                        self.save_as();
                                        ui.close();
                                    }
                                    ui.separator();
                                    ui.label(
                                        RichText::new("Layout").size(11.0).color(tokens.text_muted),
                                    );
                                    for kind in
                                        [LayoutKind::Tree, LayoutKind::Radial, LayoutKind::Logical]
                                    {
                                        if ui
                                            .selectable_label(self.kind == kind, layout_name(kind))
                                            .clicked()
                                        {
                                            self.set_layout(kind);
                                            ui.close();
                                        }
                                    }
                                    ui.separator();
                                    ui.label(
                                        RichText::new("Theme").size(11.0).color(tokens.text_muted),
                                    );
                                    for name in Palette::NAMES {
                                        if ui.selectable_label(self.theme == name, name).clicked() {
                                            self.set_theme(name);
                                            ui.close();
                                        }
                                    }
                                    ui.separator();
                                    if ui.checkbox(&mut self.prefs.grid, "Dot grid").changed() {
                                        self.prefs.save();
                                    }
                                    if ui
                                        .checkbox(&mut self.prefs.animations, "Animations")
                                        .changed()
                                    {
                                        self.applied_style = None;
                                        self.prefs.save();
                                    }
                                },
                            )
                            .response
                            .on_hover_text("View options");
                        });
                    });
            });
    }

    fn toolbar_button(
        &self,
        ui: &mut egui::Ui,
        icon: Icon,
        selected: bool,
        enabled: bool,
        tooltip: &str,
    ) -> bool {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(32.0, 30.0), Sense::click());
        let hovered = response.hovered() && enabled;
        let fill = if selected {
            self.tokens.accent_soft
        } else if hovered {
            self.tokens.surface_muted
        } else {
            Color32::TRANSPARENT
        };
        if fill.a() > 0 {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(style::RADIUS_CONTROL), fill);
        }
        let color = if !enabled {
            self.tokens.text_faint
        } else if selected {
            self.tokens.accent
        } else if hovered {
            self.tokens.text
        } else {
            self.tokens.text_muted
        };
        icons::draw(ui.painter(), rect.shrink(5.0), icon, color);
        let response = response.on_hover_text(tooltip);
        enabled && response.clicked()
    }

    fn set_layout(&mut self, kind: LayoutKind) {
        if kind == self.kind {
            return;
        }
        self.commit_editing();
        if self
            .apply("change layout", |map| {
                map.set_layout(kind);
                Ok(())
            })
            .is_some()
        {
            self.fit_pending = true;
        }
    }

    fn cycle_layout(&mut self) {
        let order = [LayoutKind::Tree, LayoutKind::Radial, LayoutKind::Logical];
        let index = order
            .iter()
            .position(|kind| *kind == self.kind)
            .unwrap_or(0);
        self.set_layout(order[(index + 1) % order.len()]);
    }

    fn set_theme(&mut self, theme: &str) {
        if theme == self.theme {
            return;
        }
        self.commit_editing();
        if self
            .apply("change theme", |map| {
                map.theme = theme.to_owned();
                Ok(())
            })
            .is_some()
        {
            self.prefs.theme = Some(theme.to_owned());
            self.prefs.save();
        }
    }

    fn cycle_theme(&mut self) {
        let index = Palette::NAMES
            .iter()
            .position(|name| *name == self.theme)
            .unwrap_or(0);
        let next = Palette::NAMES[(index + 1) % Palette::NAMES.len()];
        self.set_theme(next);
    }

    fn paint_status(&self, painter: &egui::Painter, viewport: egui::Rect) {
        let tokens = self.tokens;
        let dirty = self.editor.is_dirty();
        let file_name = self
            .path
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| self.map().title.clone());
        let file_name = truncate(&file_name, 34);
        let nodes = self.map().node_count();
        let nodes_text = if nodes == 1 {
            "1 node".to_owned()
        } else {
            format!("{nodes} nodes")
        };
        let state = if dirty { "unsaved" } else { "saved" };
        let text = format!(
            "{file_name}   ·   {state}   ·   {:.0}%   ·   {nodes_text}",
            self.zoom * 100.0
        );
        let font = FontId::proportional(style::FONT_SMALL);
        let status = painter.layout_no_wrap(text, font.clone(), tokens.text_muted);
        let error = self.error.as_ref().map(|error| {
            painter.layout(
                error.clone(),
                font,
                tokens.error,
                (viewport.width() - 48.0).clamp(180.0, 440.0),
            )
        });

        let padding = 10.0;
        let dot_width = 14.0;
        let error_height = error
            .as_ref()
            .map_or(0.0, |galley| galley.size().y + style::SPACE_1);
        let width = (status.size().x + dot_width + padding * 2.0)
            .max(
                error
                    .as_ref()
                    .map_or(0.0, |galley| galley.size().x + padding * 2.0),
            )
            .min(viewport.width() - 24.0);
        let height = status.size().y + error_height + padding * 2.0;
        let rect = egui::Rect::from_min_size(
            egui::pos2(
                viewport.center().x - width * 0.5,
                viewport.bottom() - 12.0 - height,
            ),
            egui::vec2(width, height),
        );
        let radius = CornerRadius::same(style::RADIUS_PANEL);
        let radius_soft = CornerRadius::same(style::RADIUS_PANEL.saturating_add(1));
        let shadow = tokens.status_shadow();

        painter.rect_filled(
            rect.translate(egui::vec2(
                f32::from(shadow.offset[0]),
                f32::from(shadow.offset[1]),
            ))
            .expand(f32::from(shadow.spread) + 1.0),
            radius_soft,
            style::with_alpha(shadow.color, 14),
        );
        painter.rect_filled(rect, radius, tokens.surface);
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(1.0, tokens.border),
            StrokeKind::Inside,
        );

        let line_y = rect.top() + padding + error_height;
        painter.circle_filled(
            egui::pos2(rect.left() + padding + 3.0, line_y + status.size().y * 0.5),
            3.0,
            if dirty {
                tokens.warning
            } else {
                tokens.success
            },
        );
        painter.galley(
            egui::pos2(rect.left() + padding + dot_width, line_y),
            status,
            tokens.text_muted,
        );
        if let Some(error) = error {
            painter.galley(
                egui::pos2(rect.left() + padding, rect.top() + padding),
                error,
                tokens.error,
            );
        }
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let tokens = self.tokens;
        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(tokens.bg))
            .show(ui, |ui| {
                let available = ui.available_size();
                let (response, painter) = ui.allocate_painter(available, Sense::click_and_drag());
                let viewport = response.rect;
                if viewport.width() < 2.0 || viewport.height() < 2.0 {
                    return;
                }

                if self.fit_pending {
                    self.begin_fit(ui.ctx(), viewport);
                    self.fit_pending = false;
                }
                self.update_fit_animation(ui.ctx());

                let now = ui.input(|input| input.time);
                self.track_node_births(now);
                let pointer = ui.input(|input| input.pointer.hover_pos());
                self.hovered = pointer
                    .and_then(|position| self.node_at(position, viewport))
                    .map(|node| node.id.clone());

                // Trackpad pinch zoom takes precedence; fall back to scroll-wheel zoom.
                let zoom_delta = ui.input(|input| input.zoom_delta());
                let scroll = ui.input(|input| input.smooth_scroll_delta.y);
                if response.hovered() && (zoom_delta != 1.0 || scroll.abs() > 0.01) {
                    self.fit_animation = None;
                    let pointer = pointer.unwrap_or_else(|| viewport.center());
                    let before = self.world_at(pointer, viewport);
                    let factor = if zoom_delta != 1.0 {
                        zoom_delta
                    } else {
                        (scroll * 0.0015).exp()
                    };
                    self.zoom = (self.zoom * factor).clamp(0.08, 4.0);
                    let after = self.world_at(pointer, viewport);
                    self.view_center.x += before.x - after.x;
                    self.view_center.y += before.y - after.y;
                }

                self.handle_canvas_input(ui, &response, viewport);

                self.paint_grid(&painter, viewport);
                self.paint_edges(&painter, viewport);
                self.paint_nodes(ui.ctx(), &painter, viewport, now);
                self.show_editor(ui, viewport);
                self.paint_status(&painter, viewport);
            });
    }

    fn handle_canvas_input(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        viewport: egui::Rect,
    ) {
        let pointer = ui.input(|input| input.pointer.interact_pos());

        if response.double_clicked_by(egui::PointerButton::Primary)
            && let Some(position) = pointer
            && let Some(id) = self.node_at(position, viewport).map(|node| node.id.clone())
        {
            self.selected = Some(id.clone());
            self.start_editing(id, false);
            return;
        }

        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(position) = pointer
            && let Some(node) = self.node_at(position, viewport)
        {
            let node_id = node.id.clone();
            let rect = node.rect;
            let world = self.world_at(position, viewport);
            self.selected = Some(node_id.clone());
            self.drag = Some(DragState {
                id: node_id,
                grab: mappamente_core::Point::new(world.x - rect.x, world.y - rect.y),
                start_screen: position,
                moved: false,
            });
        }

        if let Some(drag) = self.drag.as_mut()
            && let Some(position) = pointer
            && position.distance(drag.start_screen) > 3.0
        {
            drag.moved = true;
        }

        let drag_move = self.drag.as_ref().and_then(|drag| {
            if !drag.moved || !response.dragged_by(egui::PointerButton::Primary) {
                return None;
            }
            let position = pointer?;
            let world = self.world_at(position, viewport);
            Some((
                drag.id.clone(),
                world.x - drag.grab.x,
                world.y - drag.grab.y,
            ))
        });
        if let Some((id, x, y)) = drag_move {
            let now_ms = (ui.input(|input| input.time) * 1000.0) as u64;
            self.apply_coalesced("move node", now_ms, 700, |map| {
                map.set_pos(&id, Some([x, y]))
            });
        }

        if response.drag_stopped_by(egui::PointerButton::Primary)
            && let Some(drag) = self.drag.take()
        {
            if drag.moved {
                let target = pointer
                    .and_then(|position| self.drop_target_at(position, viewport, &drag.id))
                    .map(|node| node.id.clone());
                if let Some(target) = target {
                    let id = drag.id.clone();
                    if self
                        .apply("reparent node", |map| {
                            map.move_node(&id, &target, usize::MAX)?;
                            map.set_pos(&id, None)
                        })
                        .is_some()
                    {
                        self.selected = Some(id);
                    }
                }
            }
            self.selected = Some(drag.id);
        }

        if response.clicked_by(egui::PointerButton::Primary)
            && let Some(position) = pointer
        {
            self.selected = self.node_at(position, viewport).map(|node| node.id.clone());
        }

        if self.drag.is_none()
            && (response.dragged_by(egui::PointerButton::Primary)
                || response.dragged_by(egui::PointerButton::Middle))
        {
            let delta = response.drag_delta();
            self.view_center.x -= delta.x / self.zoom;
            self.view_center.y -= delta.y / self.zoom;
        }

        if let Some(position) = pointer {
            let cursor = if self.drag.is_some() {
                egui::CursorIcon::Grabbing
            } else if self.node_at(position, viewport).is_some() {
                egui::CursorIcon::Grab
            } else {
                egui::CursorIcon::Default
            };
            ui.ctx().set_cursor_icon(cursor);
        }
    }

    fn show_editor(&mut self, ui: &egui::Ui, viewport: egui::Rect) {
        let Some(id) = self.editing.as_ref().map(|state| state.id.clone()) else {
            return;
        };
        let Some(node_rect) = self.layout.get(&id).map(|node| node.rect) else {
            self.editing = None;
            return;
        };
        let screen_rect = self.to_screen_rect(node_rect, viewport);
        let position = egui::pos2(
            screen_rect.min.x.max(viewport.min.x + 8.0),
            screen_rect.min.y.max(viewport.min.y + 8.0),
        );
        let width = screen_rect.width().max(220.0);
        let font_size = (style::FONT_NODE * self.zoom).clamp(11.0, 22.0);
        let fill = self.tokens.surface;
        let border = Stroke::new(1.5, self.tokens.accent);

        let Some(edit) = self.editing.as_mut() else {
            return;
        };
        let mut commit = false;
        let mut cancel = false;

        egui::Area::new(egui::Id::unique("node_editor"))
            .order(egui::Order::Foreground)
            .fixed_pos(position)
            .show(ui.ctx(), |ui| {
                egui::Frame::default()
                    .fill(fill)
                    .stroke(border)
                    .corner_radius(egui::CornerRadius::same(style::RADIUS_CONTROL))
                    .inner_margin(egui::Margin::same(6))
                    .show(ui, |ui| {
                        ui.set_min_width(width);
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut edit.buffer)
                                .font(FontId::proportional(font_size))
                                .desired_width(width)
                                .hint_text("Node text"),
                        );
                        if edit.focus {
                            response.request_focus();
                            edit.focus = false;
                        } else if ui.input(|input| input.key_pressed(Key::Escape)) {
                            cancel = true;
                        } else if ui.input(|input| input.key_pressed(Key::Enter))
                            || response.lost_focus()
                        {
                            commit = true;
                        }
                    });
            });

        if cancel {
            self.cancel_editing();
        } else if commit {
            self.commit_editing();
        }
    }

    fn paint_grid(&self, painter: &egui::Painter, viewport: egui::Rect) {
        if !self.prefs.grid || self.zoom < 0.16 {
            return;
        }
        // Keep dots between roughly 24 and 72 screen pixels apart, so the count
        // stays bounded at every zoom level.
        let mut step = 48.0;
        while step * self.zoom < 24.0 {
            step *= 2.0;
        }
        while step * self.zoom > 72.0 && step > 12.0 {
            step *= 0.5;
        }
        let top_left = self.world_at(viewport.left_top(), viewport);
        let start_x = (top_left.x / step).floor() * step;
        let start_y = (top_left.y / step).floor() * step;
        let screen_step = (step * self.zoom).max(1.0);
        let columns = (viewport.width() / screen_step).ceil() as usize + 2;
        let rows = (viewport.height() / screen_step).ceil() as usize + 2;
        let dot = style::with_alpha(self.tokens.grid, if self.tokens.dark { 150 } else { 200 });
        for column in 0..columns {
            let x = start_x + column as f32 * step;
            for row in 0..rows {
                let y = start_y + row as f32 * step;
                let position = self.to_screen(mappamente_core::Point::new(x, y), viewport);
                painter.circle_filled(position, 1.0, dot);
            }
        }
    }

    fn paint_edges(&self, painter: &egui::Painter, viewport: egui::Rect) {
        for node in &self.layout.nodes {
            let Some(parent_id) = &node.parent else {
                continue;
            };
            let Some(parent) = self.layout.get(parent_id) else {
                continue;
            };
            let from = parent.rect.edge_toward(node.rect.center());
            let to = node.rect.edge_toward(parent.rect.center());
            let (control_a, control_b) = control_points(from, to);
            let p0 = self.to_screen(from, viewport);
            let p1 = self.to_screen(control_a, viewport);
            let p2 = self.to_screen(control_b, viewport);
            let p3 = self.to_screen(to, viewport);
            if !egui::Rect::from_two_pos(p0, p3)
                .expand(160.0)
                .intersects(viewport)
                || p0.distance(p3) < 0.5
            {
                continue;
            }

            let branch = self.branch_of.get(&node.id).copied();
            let color = self.tokens.edge_color(branch, node.depth);
            let width = (2.6 - 0.22 * node.depth as f32).max(1.1) * self.zoom.clamp(0.55, 1.8);
            painter.add(egui::Shape::CubicBezier(
                egui::epaint::CubicBezierShape::from_points_stroke(
                    [p0, p1, p2, p3],
                    false,
                    Color32::TRANSPARENT,
                    Stroke::new(width, color),
                ),
            ));
        }
    }

    fn paint_nodes(
        &self,
        context: &egui::Context,
        painter: &egui::Painter,
        viewport: egui::Rect,
        now: f64,
    ) {
        let text_size = style::FONT_NODE * self.zoom;
        let selected_id = self.selected.as_deref();
        let hovered_id = self.hovered.as_deref();
        let selection = if self.prefs.animations {
            selected_id.map_or(0.0, |id| {
                context.animate_bool_with_time(
                    egui::Id::unique(("node_selection", id)),
                    true,
                    style::ANIMATION_TIME,
                )
            })
        } else if selected_id.is_some() {
            1.0
        } else {
            0.0
        };
        let hover = if self.prefs.animations {
            hovered_id.map_or(0.0, |id| {
                context.animate_bool_with_time(
                    egui::Id::unique(("node_hover", id)),
                    true,
                    style::ANIMATION_TIME,
                )
            })
        } else if hovered_id.is_some() {
            1.0
        } else {
            0.0
        };

        for node in &self.layout.nodes {
            let screen_rect = self.to_screen_rect(node.rect, viewport);
            if !screen_rect.intersects(viewport) {
                continue;
            }

            let appear = self.node_alpha(&node.id, now);
            if appear <= 0.0 {
                continue;
            }
            let custom = self.colors.get(&node.id).copied();
            let base_fill =
                self.tokens
                    .node_fill(self.branch_of.get(&node.id).copied(), node.depth, custom);
            let base_text = self.tokens.node_text(base_fill, node.depth);
            let fill = style::fade(base_fill, appear);
            let text_color = style::fade(base_text, appear);
            let visual_rect = if node.depth == 0 {
                screen_rect.expand(1.5)
            } else {
                screen_rect
            };
            let radius = (visual_rect.height() * 0.5).clamp(3.0, style::RADIUS_NODE_MAX) as u8;
            let corner = CornerRadius::same(radius);
            let is_selected = selected_id == Some(node.id.as_str());
            let is_hovered = hovered_id == Some(node.id.as_str());

            if is_selected && selection > 0.0 {
                painter.rect_filled(
                    visual_rect.expand(4.0 * selection),
                    CornerRadius::same(radius.saturating_add(3)),
                    style::fade(self.tokens.halo(), selection),
                );
            } else if is_hovered && hover > 0.0 {
                painter.rect_filled(
                    visual_rect.expand(3.0 * hover),
                    CornerRadius::same(radius.saturating_add(2)),
                    style::fade(self.tokens.halo(), hover * 0.6),
                );
            }

            painter.rect_filled(visual_rect, corner, fill);
            if appear > 0.15 {
                painter.rect_stroke(
                    visual_rect,
                    corner,
                    Stroke::new(
                        1.0,
                        style::fade(style::mix(base_fill, self.tokens.text, 0.16), appear),
                    ),
                    StrokeKind::Inside,
                );
            }

            if is_selected && selection > 0.0 {
                painter.rect_stroke(
                    visual_rect.expand(1.0 + selection),
                    CornerRadius::same(radius.saturating_add(1)),
                    Stroke::new(
                        (1.2 + 0.8 * selection).min(2.0),
                        style::with_alpha(self.tokens.accent, (255.0 * selection) as u8),
                    ),
                    StrokeKind::Outside,
                );
            }

            let editing_this = self
                .editing
                .as_ref()
                .is_some_and(|state| state.id == node.id);
            if text_size >= 5.0 && !editing_this && appear > 0.05 {
                let font_id = FontId::proportional(text_size);
                let row_height = context.fonts_mut(|fonts| fonts.row_height(&font_id));
                let pad_x = 14.0;
                let text_block_height = node.lines.len() as f32 * row_height;
                let top_y = node.rect.y + (node.rect.h - text_block_height) / 2.0;
                for (index, line) in node.lines.iter().enumerate() {
                    if line.is_empty() {
                        continue;
                    }
                    let position = self.to_screen(
                        mappamente_core::Point::new(
                            node.rect.x + pad_x,
                            top_y + index as f32 * row_height + row_height * 0.5,
                        ),
                        viewport,
                    );
                    painter.text(
                        position,
                        Align2::LEFT_CENTER,
                        line,
                        font_id.clone(),
                        text_color,
                    );
                }
            }

            if node.collapsed {
                let position = self.to_screen(
                    mappamente_core::Point::new(node.rect.right() - 10.0, node.rect.center().y),
                    viewport,
                );
                painter.circle_filled(
                    position,
                    (4.0 * self.zoom).clamp(1.5, 5.0),
                    style::fade(style::mix(base_fill, base_text, 0.75), appear),
                );
            }
        }
    }

    fn begin_fit(&mut self, context: &egui::Context, viewport: egui::Rect) {
        let bounds = self.layout.bounds;
        let width = bounds.w.max(1.0);
        let height = bounds.h.max(1.0);
        let margin = 96.0;
        let scale_x = (viewport.width() - margin * 2.0).max(20.0) / width;
        let scale_y = (viewport.height() - margin * 2.0).max(20.0) / height;
        let zoom = scale_x.min(scale_y).clamp(0.02, 3.0);
        let center = bounds.center();
        if self.prefs.animations
            && ((zoom - self.zoom).abs() > 0.001
                || (center.x - self.view_center.x).abs() > 0.5
                || (center.y - self.view_center.y).abs() > 0.5)
        {
            self.fit_animation = Some(FitAnimation {
                start_time: context.input(|input| input.time),
                from_zoom: self.zoom,
                to_zoom: zoom,
                from_center: self.view_center,
                to_center: center,
            });
            context.request_repaint();
        } else {
            self.fit_animation = None;
            self.zoom = zoom;
            self.view_center = center;
        }
    }

    fn update_fit_animation(&mut self, context: &egui::Context) {
        let Some(animation) = &self.fit_animation else {
            return;
        };
        let elapsed = context.input(|input| input.time) - animation.start_time;
        let duration = f64::from(style::ANIMATION_TIME) * 1.35;
        let progress = (elapsed / duration).clamp(0.0, 1.0) as f32;
        let eased = 1.0 - (1.0 - progress).powi(3);
        self.zoom = animation.from_zoom + (animation.to_zoom - animation.from_zoom) * eased;
        self.view_center = mappamente_core::Point::new(
            animation.from_center.x + (animation.to_center.x - animation.from_center.x) * eased,
            animation.from_center.y + (animation.to_center.y - animation.from_center.y) * eased,
        );
        if progress >= 1.0 {
            self.fit_animation = None;
        } else {
            context.request_repaint();
        }
    }

    fn track_node_births(&mut self, now: f64) {
        if !self.nodes_initialized {
            self.known_nodes = self
                .layout
                .nodes
                .iter()
                .map(|node| node.id.clone())
                .collect();
            self.nodes_initialized = true;
        } else if self.prefs.animations {
            for node in &self.layout.nodes {
                if !self.known_nodes.contains(&node.id) {
                    self.known_nodes.insert(node.id.clone());
                    self.node_birth.insert(node.id.clone(), now);
                }
            }
        }
        self.node_birth.retain(|_, at| now - *at < 0.35);
    }

    fn node_alpha(&self, id: &str, now: f64) -> f32 {
        if !self.prefs.animations {
            return 1.0;
        }
        let Some(birth) = self.node_birth.get(id) else {
            return 1.0;
        };
        ((now - birth) / f64::from(style::ANIMATION_TIME)).clamp(0.0, 1.0) as f32
    }

    fn world_at(&self, screen: egui::Pos2, viewport: egui::Rect) -> mappamente_core::Point {
        mappamente_core::Point::new(
            self.view_center.x + (screen.x - viewport.center().x) / self.zoom,
            self.view_center.y + (screen.y - viewport.center().y) / self.zoom,
        )
    }

    fn to_screen(&self, point: mappamente_core::Point, viewport: egui::Rect) -> egui::Pos2 {
        egui::pos2(
            viewport.center().x + (point.x - self.view_center.x) * self.zoom,
            viewport.center().y + (point.y - self.view_center.y) * self.zoom,
        )
    }

    fn to_screen_rect(&self, rect: mappamente_core::Rect, viewport: egui::Rect) -> egui::Rect {
        let min = self.to_screen(mappamente_core::Point::new(rect.x, rect.y), viewport);
        egui::Rect::from_min_size(min, egui::vec2(rect.w * self.zoom, rect.h * self.zoom))
    }

    fn node_at(&self, position: egui::Pos2, viewport: egui::Rect) -> Option<&LayoutNode> {
        self.layout
            .nodes
            .iter()
            .rev()
            .find(|node| self.to_screen_rect(node.rect, viewport).contains(position))
    }

    /// Finds the node under `position`, ignoring the dragged branch.
    ///
    /// While dragging, the moving node follows the pointer and covers the
    /// intended drop target, so the target must be searched underneath it.
    fn drop_target_at(
        &self,
        position: egui::Pos2,
        viewport: egui::Rect,
        dragged: &str,
    ) -> Option<&LayoutNode> {
        self.layout.nodes.iter().rev().find(|node| {
            node.id != dragged
                && !is_descendant(self.map(), dragged, &node.id)
                && self.to_screen_rect(node.rect, viewport).contains(position)
        })
    }
}

impl eframe::App for MappamenteApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.handle_close(ui.ctx());
        self.apply_visuals(ui.ctx());
        self.handle_keys(ui.ctx());

        let dropped = ui.ctx().input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .next()
        });
        if let Some(path) = dropped {
            self.load(path);
        }

        if self.ai_open {
            let tokens = self.tokens;
            let response = {
                let map = self.editor.map();
                egui::Panel::right("mappamente_ai")
                    .resizable(true)
                    .default_size(380.0)
                    .min_size(320.0)
                    .max_size(560.0)
                    .frame(
                        egui::Frame::default()
                            .fill(tokens.bg)
                            .stroke(Stroke::new(1.0, tokens.border))
                            .inner_margin(egui::Margin::symmetric(14, 12)),
                    )
                    .show(ui, |ui| self.ai.ui(ui, map, &tokens))
            };
            self.ai_width = response.response.rect.width();
            if let Some(action) = response.inner {
                self.handle_ai_action(action);
            }
        }
        self.toolbar(ui);
        self.canvas(ui);
        self.update_window_title(ui.ctx());
    }
}

fn compute_layout(map: &MindMap, kind: LayoutKind) -> Layout {
    Layout::compute_with(map, kind, Metrics::default(), LayoutOptions::default())
}

fn collect_colors(map: &MindMap) -> HashMap<String, Color32> {
    fn walk(node: &Node, out: &mut HashMap<String, Color32>) {
        if let Some(value) = &node.color
            && let Some([r, g, b, a]) = parse_hex(value)
        {
            out.insert(node.id.clone(), Color32::from_rgba_unmultiplied(r, g, b, a));
        }
        for child in &node.children {
            walk(child, out);
        }
    }
    let mut out = HashMap::with_capacity(map.node_count());
    walk(&map.root, &mut out);
    out
}

/// Maps every node to the index of its first-level branch.
fn collect_branches(map: &MindMap) -> HashMap<String, usize> {
    fn walk(node: &Node, branch: usize, out: &mut HashMap<String, usize>) {
        for child in &node.children {
            out.insert(child.id.clone(), branch);
            walk(child, branch, out);
        }
    }
    let mut out = HashMap::with_capacity(map.node_count());
    for (branch, child) in map.root.children.iter().enumerate() {
        out.insert(child.id.clone(), branch);
        walk(child, branch, &mut out);
    }
    out
}

fn is_descendant(map: &MindMap, ancestor: &str, candidate: &str) -> bool {
    fn contains(node: &Node, id: &str) -> bool {
        node.id == id || node.children.iter().any(|child| contains(child, id))
    }
    map.find(ancestor)
        .is_some_and(|node| contains(node, candidate))
}

fn layout_name(kind: LayoutKind) -> &'static str {
    match kind {
        LayoutKind::Tree => "Tree",
        LayoutKind::Radial => "Radial",
        LayoutKind::Logical => "Logical",
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn sanitize_file_name(title: &str) -> String {
    let mut out: String = title
        .chars()
        .map(|character| {
            if matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            ) {
                '-'
            } else {
                character
            }
        })
        .collect();
    out = out.trim().trim_matches('.').to_owned();
    if out.is_empty() {
        "untitled".to_owned()
    } else {
        out
    }
}

fn control_points(
    from: mappamente_core::Point,
    to: mappamente_core::Point,
) -> (mappamente_core::Point, mappamente_core::Point) {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    if dx.abs() >= dy.abs() {
        (
            mappamente_core::Point::new(from.x + dx * 0.5, from.y),
            mappamente_core::Point::new(to.x - dx * 0.5, to.y),
        )
    } else {
        (
            mappamente_core::Point::new(from.x, from.y + dy * 0.5),
            mappamente_core::Point::new(to.x, to.y - dy * 0.5),
        )
    }
}

fn welcome_map(theme: &str) -> MindMap {
    let mut map = MindMap::new("Welcome to Mappamente", "Mappamente");
    map.theme = theme.to_owned();
    map.layout = LayoutKind::Tree;

    let root = map.root.id.clone();
    let create = add(&mut map, &root, "Create");
    add(&mut map, &create, "Tab: add a child");
    add(&mut map, &create, "Enter: add a sibling");
    add(&mut map, &create, "Double-click: edit text");

    let navigate = add(&mut map, &root, "Navigate");
    add(&mut map, &navigate, "Drag the canvas to pan");
    add(&mut map, &navigate, "Scroll to zoom");
    add(&mut map, &navigate, "Fit to frame everything");

    let organize = add(&mut map, &root, "Organize");
    add(&mut map, &organize, "Drag a node to move it");
    add(&mut map, &organize, "Drop it on another to reparent");
    add(&mut map, &organize, "Delete removes a branch");

    let files = add(&mut map, &root, "Files");
    add(&mut map, &files, "Native .mappa JSON");
    add(&mut map, &files, "Atomic saves");
    add(&mut map, &files, "Markdown import");

    let next = add(&mut map, &root, "Next");
    add(&mut map, &next, "MCP server");
    add(&mut map, &next, "AI panel");
    add(&mut map, &next, "More layouts");

    map
}

fn add(map: &mut MindMap, parent: &str, text: &str) -> String {
    map.add_child(parent, text)
        .expect("adding nodes to a fresh map cannot fail")
}

/// Draws the Mappamente mark as RGBA pixels for the window/taskbar icon.
fn window_icon() -> egui::IconData {
    const SIZE: usize = 128;
    let mut canvas = IconCanvas::new(SIZE);
    canvas.fill_rounded_rect(0.0, 0.0, 128.0, 128.0, 30.0, [14, 16, 20, 255]);
    // The mark is an M drawn as a route between nodes.
    let points = [
        (24.0, 98.0),
        (24.0, 34.0),
        (64.0, 72.0),
        (104.0, 34.0),
        (104.0, 98.0),
    ];
    for pair in points.windows(2) {
        canvas.line(
            pair[0].0,
            pair[0].1,
            pair[1].0,
            pair[1].1,
            4.5,
            [79, 140, 255, 255],
        );
    }
    canvas.fill_circle(24.0, 34.0, 7.0, [79, 140, 255, 255]);
    canvas.fill_circle(104.0, 34.0, 7.0, [45, 212, 191, 255]);
    canvas.fill_circle(64.0, 72.0, 9.0, [232, 236, 244, 255]);
    canvas.fill_circle(24.0, 98.0, 5.0, [232, 236, 244, 255]);
    canvas.fill_circle(104.0, 98.0, 5.0, [232, 236, 244, 255]);
    canvas.into_icon()
}

struct IconCanvas {
    size: usize,
    pixels: Vec<u8>,
}

impl IconCanvas {
    fn new(size: usize) -> Self {
        Self {
            size,
            pixels: vec![0; size * size * 4],
        }
    }

    fn set(&mut self, x: i32, y: i32, color: [u8; 4]) {
        if x < 0 || y < 0 {
            return;
        }
        let (x, y) = (x as usize, y as usize);
        if x >= self.size || y >= self.size {
            return;
        }
        let index = (y * self.size + x) * 4;
        self.pixels[index..index + 4].copy_from_slice(&color);
    }

    fn fill_circle(&mut self, cx: f32, cy: f32, radius: f32, color: [u8; 4]) {
        let min_x = (cx - radius).floor() as i32;
        let max_x = (cx + radius).ceil() as i32;
        let min_y = (cy - radius).floor() as i32;
        let max_y = (cy + radius).ceil() as i32;
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let dx = x as f32 - cx;
                let dy = y as f32 - cy;
                if dx * dx + dy * dy <= radius * radius {
                    self.set(x, y, color);
                }
            }
        }
    }

    fn fill_rounded_rect(
        &mut self,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        radius: f32,
        color: [u8; 4],
    ) {
        for y in (y0.floor() as i32)..=(y1.ceil() as i32) {
            for x in (x0.floor() as i32)..=(x1.ceil() as i32) {
                let px = x as f32;
                let py = y as f32;
                if !(x0..=x1).contains(&px) || !(y0..=y1).contains(&py) {
                    continue;
                }
                let nearest_x = px.clamp(x0 + radius, x1 - radius);
                let nearest_y = py.clamp(y0 + radius, y1 - radius);
                let dx = px - nearest_x;
                let dy = py - nearest_y;
                if dx * dx + dy * dy <= radius * radius {
                    self.set(x, y, color);
                }
            }
        }
    }

    fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: [u8; 4]) {
        let dx = x1 - x0;
        let dy = y1 - y0;
        let length = (dx * dx + dy * dy).sqrt();
        let steps = (length * 2.0).ceil().max(1.0) as i32;
        for step in 0..=steps {
            let t = step as f32 / steps as f32;
            self.fill_circle(x0 + dx * t, y0 + dy * t, width / 2.0, color);
        }
    }

    fn into_icon(self) -> egui::IconData {
        egui::IconData {
            rgba: self.pixels,
            width: self.size as u32,
            height: self.size as u32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_file_names() {
        assert_eq!(sanitize_file_name("My map"), "My map");
        assert_eq!(sanitize_file_name("a/b:c*?"), "a-b-c--");
        assert_eq!(sanitize_file_name("..."), "untitled");
        assert_eq!(sanitize_file_name("  "), "untitled");
    }

    #[test]
    fn descendant_detection() {
        let mut map = MindMap::new("Test", "Root");
        let child = map.add_child(&map.root.id.clone(), "Child").unwrap();
        let grandchild = map.add_child(&child, "Grandchild").unwrap();
        assert!(is_descendant(&map, &map.root.id, &child));
        assert!(is_descendant(&map, &child, &grandchild));
        assert!(!is_descendant(&map, &grandchild, &child));
    }

    #[test]
    fn branches_follow_first_level_ancestors() {
        let mut map = MindMap::new("Test", "Root");
        let root = map.root.id.clone();
        let first = map.add_child(&root, "First").unwrap();
        let first_child = map.add_child(&first, "First child").unwrap();
        let second = map.add_child(&root, "Second").unwrap();
        let second_child = map.add_child(&second, "Second child").unwrap();
        let branches = collect_branches(&map);
        assert_eq!(branches.get(&first), Some(&0));
        assert_eq!(branches.get(&first_child), Some(&0));
        assert_eq!(branches.get(&second), Some(&1));
        assert_eq!(branches.get(&second_child), Some(&1));
        assert!(!branches.contains_key(&root));
    }

    #[test]
    fn explicit_node_colors_are_parsed() {
        let mut map = MindMap::new("Test", "Root");
        let child = map.add_child(&map.root.id.clone(), "Child").unwrap();
        map.set_color(&child, Some("#112233")).unwrap();
        let colors = collect_colors(&map);
        assert_eq!(
            colors.get(&child),
            Some(&Color32::from_rgb(0x11, 0x22, 0x33))
        );
    }

    #[test]
    fn long_status_names_are_truncated() {
        assert_eq!(truncate("demo.mappa", 34), "demo.mappa");
        let long = "a".repeat(40);
        let short = truncate(&long, 10);
        assert_eq!(short.chars().count(), 10);
        assert!(short.ends_with('…'));
    }
}
