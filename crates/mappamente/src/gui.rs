//! Editing canvas for `.mappa` documents.
//!
//! Double-click a node to edit its label, `Tab` adds a child, `Enter` adds a
//! sibling, `Delete` removes a branch, drag a node to move or reparent it, and
//! drag the background to pan. All changes go through [`Editor`], so undo,
//! redo, coalesced drags and dirty tracking are shared with the core.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use eframe::egui::{
    self, Align, Align2, Color32, ComboBox, CornerRadius, FontId, Key, Modifiers, RichText, Sense,
    Stroke, StrokeKind,
};
use mappamente_core::Editor;
use mappamente_core::io;
use mappamente_core::layout::{Layout, LayoutNode, LayoutOptions, Metrics};
use mappamente_core::model::{LayoutKind, MindMap, Node, NodeId};
use mappamente_core::theme::{Palette, parse_hex};

const APP_NAME: &str = "Mappamente";
const NODE_FONT: f32 = 15.0;
const LINE_HEIGHT: f32 = 20.0;

/// Opens the editor, optionally loading `initial`.
pub fn run(initial: Option<PathBuf>) -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 780.0])
            .with_min_inner_size([720.0, 480.0])
            .with_title(APP_NAME)
            .with_icon(window_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "mappamente",
        options,
        Box::new(move |creation| {
            configure(&creation.egui_ctx);
            Ok(Box::new(MappamenteApp::new(initial.clone())))
        }),
    )
    .map_err(|error| anyhow::anyhow!("cannot open the Mappamente window: {error}"))
}

fn configure(context: &egui::Context) {
    context.all_styles_mut(|style| {
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    });
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

struct MappamenteApp {
    editor: Editor,
    path: Option<PathBuf>,
    kind: LayoutKind,
    theme: String,
    palette: Palette,
    applied_theme: Option<String>,
    layout: Layout,
    colors: HashMap<String, String>,
    zoom: f32,
    view_center: mappamente_core::Point,
    fit_pending: bool,
    error: Option<String>,
    selected: Option<NodeId>,
    editing: Option<EditingState>,
    drag: Option<DragState>,
    window_title: String,
    allow_close: bool,
}

impl MappamenteApp {
    fn new(initial: Option<PathBuf>) -> Self {
        let mut error = None;
        let (map, path) = match initial {
            Some(path) => match io::load_path(&path) {
                Ok(map) => (map, Some(path)),
                Err(load_error) => {
                    error = Some(format!("Could not open {}: {load_error}", path.display()));
                    (welcome_map(), None)
                }
            },
            None => (welcome_map(), None),
        };

        let selected = Some(map.root.id.clone());
        let kind = map.layout;
        let palette = Palette::by_name(&map.theme);
        let theme = palette.name.to_owned();
        let layout = compute_layout(&map, kind);
        let colors = collect_colors(&map);
        let editor = Editor::new(map);

        Self {
            editor,
            path,
            kind,
            theme,
            palette,
            applied_theme: None,
            layout,
            colors,
            zoom: 1.0,
            view_center: mappamente_core::Point::ZERO,
            fit_pending: true,
            error,
            selected,
            editing: None,
            drag: None,
            window_title: String::new(),
            allow_close: false,
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
                self.palette = Palette::by_name(&map.theme);
                self.theme = self.palette.name.to_owned();
                self.layout = compute_layout(&map, self.kind);
                self.colors = collect_colors(&map);
                self.selected = Some(map.root.id.clone());
                self.editor = Editor::new(map);
                self.path = Some(path);
                self.error = None;
                self.editing = None;
                self.drag = None;
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
        let map = MindMap::new("Untitled map", "Central idea");
        let root = map.root.id.clone();
        self.kind = map.layout;
        self.palette = Palette::by_name(&map.theme);
        self.theme = self.palette.name.to_owned();
        self.editor = Editor::new(map);
        self.editor.mark_dirty();
        self.path = None;
        self.selected = Some(root);
        self.error = None;
        self.editing = None;
        self.drag = None;
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
        self.palette = Palette::by_name(&self.editor.map().theme);
        self.theme = self.palette.name.to_owned();

        let selected = self.selected.clone();
        let editing = self.editing.as_ref().map(|state| state.id.clone());
        let drag = self.drag.as_ref().map(|state| state.id.clone());

        let (colors, layout, selected_valid, editing_valid, drag_valid) = {
            let map = self.map();
            (
                collect_colors(map),
                compute_layout(map, self.kind),
                selected.as_deref().is_some_and(|id| map.find(id).is_some()),
                editing.as_deref().is_some_and(|id| map.find(id).is_some()),
                drag.as_deref().is_some_and(|id| map.find(id).is_some()),
            )
        };

        self.colors = colors;
        self.layout = layout;
        if selected.is_some() && !selected_valid {
            self.selected = None;
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
        if self.editing.is_some() {
            return;
        }

        let command_shift = Modifiers {
            command: true,
            shift: true,
            ..Default::default()
        };
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
        if self.applied_theme.as_deref() == Some(self.theme.as_str()) {
            return;
        }
        let light = matches!(self.theme.as_str(), "light" | "paper");
        context.set_theme(if light {
            egui::ThemePreference::Light
        } else {
            egui::ThemePreference::Dark
        });
        context.all_styles_mut(|style| {
            style.visuals.panel_fill = color(self.palette.toolbar);
            style.visuals.window_fill = color(self.palette.toolbar);
            style.visuals.extreme_bg_color = color(self.palette.background);
            style.visuals.faint_bg_color = color(self.palette.grid);
            style.visuals.selection.bg_fill = color(self.palette.accent).gamma_multiply(0.55);
        });
        self.applied_theme = Some(self.theme.clone());
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        let node_count = self.map().node_count();
        let dirty = self.editor.is_dirty();
        let file_name = self
            .path
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| self.map().title.clone());

        egui::Panel::top("mappamente_top")
            .frame(
                egui::Frame::default()
                    .fill(color(palette.toolbar))
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(APP_NAME)
                            .color(color(palette.accent))
                            .strong()
                            .size(18.0),
                    );
                    ui.add_space(4.0);

                    if ui.button("New").clicked() {
                        self.new_map();
                    }
                    if ui.button("Open…").clicked() {
                        self.pick_file();
                    }
                    if ui.button("Save").clicked() {
                        self.save();
                    }
                    if ui.button("Save as…").clicked() {
                        self.save_as();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(
                            self.editing.is_none() && self.editor.can_undo(),
                            egui::Button::new("Undo"),
                        )
                        .clicked()
                    {
                        self.undo();
                    }
                    if ui
                        .add_enabled(
                            self.editing.is_none() && self.editor.can_redo(),
                            egui::Button::new("Redo"),
                        )
                        .clicked()
                    {
                        self.redo();
                    }
                    if ui.button("Fit").clicked() {
                        self.fit_pending = true;
                    }

                    ui.separator();
                    ui.label("Layout");
                    let previous_kind = self.kind;
                    ComboBox::from_id_salt("layout_combo")
                        .selected_text(layout_name(self.kind))
                        .width(88.0)
                        .show_ui(ui, |ui| {
                            for kind in [LayoutKind::Tree, LayoutKind::Radial, LayoutKind::Logical]
                            {
                                ui.selectable_value(&mut self.kind, kind, layout_name(kind));
                            }
                        });
                    if self.kind != previous_kind {
                        let kind = self.kind;
                        self.commit_editing();
                        self.apply("change layout", |map| {
                            map.set_layout(kind);
                            Ok(())
                        });
                        self.fit_pending = true;
                    }

                    ui.label("Theme");
                    let previous_theme = self.theme.clone();
                    ComboBox::from_id_salt("theme_combo")
                        .selected_text(self.theme.clone())
                        .width(82.0)
                        .show_ui(ui, |ui| {
                            for name in Palette::NAMES {
                                ui.selectable_value(&mut self.theme, name.to_owned(), name);
                            }
                        });
                    if self.theme != previous_theme {
                        let theme = self.theme.clone();
                        self.commit_editing();
                        self.apply("change theme", |map| {
                            map.theme = theme;
                            Ok(())
                        });
                        self.applied_theme = None;
                    }

                    ui.separator();
                    let label = if dirty {
                        format!("{file_name} •")
                    } else {
                        file_name
                    };
                    ui.label(RichText::new(label).color(color(palette.muted_text)));

                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{node_count} nodes"))
                                .color(color(palette.muted_text)),
                        );
                    });
                });

                if let Some(error) = &self.error {
                    ui.label(
                        RichText::new(error)
                            .color(Color32::from_rgb(180, 60, 60))
                            .size(13.0),
                    );
                }
            });
    }

    fn bottom_bar(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        let status = format!(
            "{} nodes · {} · zoom {:.0}%{}",
            self.map().node_count(),
            layout_name(self.kind),
            self.zoom * 100.0,
            if self.editor.is_dirty() {
                " · unsaved"
            } else {
                ""
            }
        );
        egui::Panel::bottom("mappamente_bottom")
            .frame(
                egui::Frame::default()
                    .fill(color(palette.toolbar))
                    .inner_margin(egui::Margin::symmetric(12, 5)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(status)
                            .color(color(palette.muted_text))
                            .size(12.0),
                    );
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(
                                "Double-click: edit · Tab: child · Enter: sibling · Del: remove · Ctrl+S: save",
                            )
                            .color(color(palette.muted_text))
                            .size(12.0),
                        );
                    });
                });
            });
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let palette = self.palette;
        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(color(palette.background)))
            .show(ui, |ui| {
                let available = ui.available_size();
                let (response, painter) = ui.allocate_painter(available, Sense::click_and_drag());
                let viewport = response.rect;
                if viewport.width() < 2.0 || viewport.height() < 2.0 {
                    return;
                }

                if self.fit_pending {
                    self.fit(viewport);
                    self.fit_pending = false;
                }

                let scroll = ui.input(|input| input.smooth_scroll_delta.y);
                if response.hovered() && scroll.abs() > 0.01 {
                    let pointer = ui
                        .input(|input| input.pointer.hover_pos())
                        .unwrap_or_else(|| viewport.center());
                    let before = self.world_at(pointer, viewport);
                    self.zoom = (self.zoom * (scroll * 0.0015).exp()).clamp(0.08, 4.0);
                    let after = self.world_at(pointer, viewport);
                    self.view_center.x += before.x - after.x;
                    self.view_center.y += before.y - after.y;
                }

                self.handle_canvas_input(ui, &response, viewport);

                self.paint_grid(&painter, viewport);
                self.paint_edges(&painter, viewport);
                self.paint_nodes(&painter, viewport);
                self.show_editor(ui, viewport);
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
        let font_size = (NODE_FONT * self.zoom).clamp(11.0, 22.0);
        let fill = color(self.palette.toolbar);
        let border = Stroke::new(1.5, color(self.palette.accent));

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
        if self.zoom < 0.35 {
            return;
        }
        let step = 48.0;
        let top_left = self.world_at(viewport.left_top(), viewport);
        let bottom_right = self.world_at(viewport.right_bottom(), viewport);
        let start_x = (top_left.x / step).floor() * step;
        let start_y = (top_left.y / step).floor() * step;
        let dot = color(self.palette.grid);
        let mut x = start_x;
        let mut columns = 0;
        while x <= bottom_right.x && columns < 300 {
            let mut y = start_y;
            let mut rows = 0;
            while y <= bottom_right.y && rows < 300 {
                let position = self.to_screen(mappamente_core::Point::new(x, y), viewport);
                painter.circle_filled(position, 1.1, dot);
                y += step;
                rows += 1;
            }
            x += step;
            columns += 1;
        }
    }

    fn paint_edges(&self, painter: &egui::Painter, viewport: egui::Rect) {
        let stroke_color = color(self.palette.edge);
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
                .expand(120.0)
                .intersects(viewport)
                && p0.distance(p3) >= 1.0
            {
                continue;
            }

            let mut points = Vec::with_capacity(17);
            for index in 0..=16 {
                let t = index as f32 / 16.0;
                points.push(cubic_point(p0, p1, p2, p3, t));
            }
            let width = (2.4 - 0.22 * node.depth as f32).max(1.2) * self.zoom.clamp(0.55, 1.8);
            painter.add(egui::Shape::line(points, Stroke::new(width, stroke_color)));
        }
    }

    fn paint_nodes(&self, painter: &egui::Painter, viewport: egui::Rect) {
        let text_size = NODE_FONT * self.zoom;
        for node in &self.layout.nodes {
            let screen_rect = self.to_screen_rect(node.rect, viewport);
            if !screen_rect.intersects(viewport) {
                continue;
            }

            let custom = self.colors.get(&node.id).map(String::as_str);
            let fill_hex = custom.unwrap_or_else(|| self.palette.node_fill(node.depth));
            let fill = color_hex(fill_hex);
            let text_color = if custom.is_some() {
                color_hex(mappamente_core::theme::readable_text(fill_hex))
            } else if node.depth == 0 {
                color(self.palette.accent_text)
            } else {
                color(self.palette.text)
            };

            let radius = (10.0 * self.zoom).clamp(2.0, 16.0) as u8;
            painter.rect_filled(screen_rect, CornerRadius::same(radius), fill);
            painter.rect_stroke(
                screen_rect,
                CornerRadius::same(radius),
                Stroke::new(
                    (1.0 * self.zoom).clamp(0.6, 1.4),
                    color(self.palette.edge).gamma_multiply(0.55),
                ),
                StrokeKind::Inside,
            );

            if self.selected.as_ref() == Some(&node.id) {
                painter.rect_stroke(
                    screen_rect.expand(2.5),
                    CornerRadius::same(radius.saturating_add(1)),
                    Stroke::new(2.0, color(self.palette.accent)),
                    StrokeKind::Middle,
                );
            }

            let editing_this = self
                .editing
                .as_ref()
                .is_some_and(|state| state.id == node.id);
            if text_size >= 5.0 && !editing_this {
                for (index, line) in node.lines.iter().enumerate() {
                    if line.is_empty() {
                        continue;
                    }
                    let position = self.to_screen(
                        mappamente_core::Point::new(
                            node.rect.x + 14.0,
                            node.rect.y + 9.0 + index as f32 * LINE_HEIGHT,
                        ),
                        viewport,
                    );
                    painter.text(
                        position,
                        Align2::LEFT_TOP,
                        line,
                        FontId::proportional(text_size),
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
                    text_color.gamma_multiply(0.75),
                );
            }
        }
    }

    fn fit(&mut self, viewport: egui::Rect) {
        let bounds = self.layout.bounds;
        let width = bounds.w.max(1.0);
        let height = bounds.h.max(1.0);
        let margin = 80.0;
        let scale_x = (viewport.width() - margin * 2.0).max(20.0) / width;
        let scale_y = (viewport.height() - margin * 2.0).max(20.0) / height;
        self.zoom = scale_x.min(scale_y).clamp(0.02, 3.0);
        self.view_center = bounds.center();
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

        self.top_bar(ui);
        self.bottom_bar(ui);
        self.canvas(ui);
        self.update_window_title(ui.ctx());
    }
}

fn compute_layout(map: &MindMap, kind: LayoutKind) -> Layout {
    Layout::compute_with(map, kind, Metrics::default(), LayoutOptions::default())
}

fn collect_colors(map: &MindMap) -> HashMap<String, String> {
    fn walk(node: &Node, out: &mut HashMap<String, String>) {
        if let Some(color) = &node.color
            && parse_hex(color).is_some()
        {
            out.insert(node.id.clone(), color.clone());
        }
        for child in &node.children {
            walk(child, out);
        }
    }
    let mut out = HashMap::with_capacity(map.node_count());
    walk(&map.root, &mut out);
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

fn cubic_point(
    p0: egui::Pos2,
    p1: egui::Pos2,
    p2: egui::Pos2,
    p3: egui::Pos2,
    t: f32,
) -> egui::Pos2 {
    let u = 1.0 - t;
    egui::pos2(
        u * u * u * p0.x + 3.0 * u * u * t * p1.x + 3.0 * u * t * t * p2.x + t * t * t * p3.x,
        u * u * u * p0.y + 3.0 * u * u * t * p1.y + 3.0 * u * t * t * p2.y + t * t * t * p3.y,
    )
}

fn color(hex: &str) -> Color32 {
    color_hex(hex)
}

fn color_hex(hex: &str) -> Color32 {
    if let Some([r, g, b, a]) = parse_hex(hex) {
        Color32::from_rgba_unmultiplied(r, g, b, a)
    } else {
        Color32::GRAY
    }
}

fn welcome_map() -> MindMap {
    let mut map = MindMap::new("Welcome to Mappamente", "Mappamente");
    map.theme = "light".to_owned();
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
}
