//! AI assistant side panel.
//!
//! Network calls run on a worker thread; the UI polls a channel and applies a
//! parsed plan through the app's `Editor`, so the canvas updates immediately and
//! every turn is one undo step.

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use eframe::egui::{self, Align, Key, Margin, RichText, Stroke, StrokeKind};
use mappamente_ai::{
    AiError, ChatMessage, ChatRequest, ChatResponse, KeySource, PRESETS, Plan, ProviderClient,
    ProviderPreset, ProviderSettings, Role, clear_api_key, key_source, load_api_key, parse_plan,
    save_api_key, system_prompt,
};
use mappamente_core::MindMap;

use crate::style::{self, Tokens};

const WELCOME: &str = "Ciao! Posso creare, riordinare o riscrivere la mappa. Le modifiche vengono applicate subito e puoi annullarle con Ctrl+Z.";

/// An action the app must apply to its live document.
pub(crate) enum AiPanelAction {
    /// Apply `plan` as one undoable edit.
    Apply {
        /// The assistant's reply, shown after applying.
        reply: String,
        /// Parsed actions.
        plan: Plan,
    },
}

enum WorkerEvent {
    Finished(mappamente_ai::Result<ChatResponse>),
}

enum PollState {
    Ready(WorkerEvent),
    Empty,
    Disconnected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EntryKind {
    User,
    Assistant,
    Status,
    Error,
}

struct ChatEntry {
    kind: EntryKind,
    text: String,
}

impl ChatEntry {
    fn user(text: impl Into<String>) -> Self {
        Self {
            kind: EntryKind::User,
            text: text.into(),
        }
    }

    fn assistant(text: impl Into<String>) -> Self {
        Self {
            kind: EntryKind::Assistant,
            text: text.into(),
        }
    }

    fn status(text: impl Into<String>) -> Self {
        Self {
            kind: EntryKind::Status,
            text: text.into(),
        }
    }

    fn error(text: impl Into<String>) -> Self {
        Self {
            kind: EntryKind::Error,
            text: text.into(),
        }
    }
}

/// State of the AI side panel.
pub(crate) struct AiPanel {
    provider_index: usize,
    base_url: String,
    model: String,
    api_key_input: String,
    key_status: KeySource,
    initialized: bool,
    messages: Vec<ChatEntry>,
    input: String,
    busy: bool,
    error: Option<String>,
    receiver: Option<Receiver<WorkerEvent>>,
    usage: Option<(u64, u64)>,
}

impl AiPanel {
    /// Creates the panel with the first provider preset selected.
    #[must_use]
    pub(crate) fn new() -> Self {
        let preset = &PRESETS[0];
        Self {
            provider_index: 0,
            base_url: preset.base_url.to_owned(),
            model: preset.model.to_owned(),
            api_key_input: String::new(),
            key_status: KeySource::Missing,
            initialized: false,
            messages: vec![ChatEntry::status(WELCOME)],
            input: String::new(),
            busy: false,
            error: None,
            receiver: None,
            usage: None,
        }
    }

    /// Draws the panel and returns an edit to apply, if one is ready.
    pub(crate) fn ui(
        &mut self,
        ui: &mut egui::Ui,
        map: &MindMap,
        tokens: &Tokens,
    ) -> Option<AiPanelAction> {
        if !self.initialized {
            self.initialized = true;
            self.key_status = key_source(self.preset());
        }
        let ready = self.poll(ui.ctx());
        self.header(ui, tokens);
        self.settings(ui, tokens);
        ui.add_space(style::SPACE_2);
        egui::Panel::bottom("ai_composer")
            .frame(egui::Frame::NONE)
            .show_separator_line(false)
            .show(ui, |ui| self.composer(ui, map, tokens));
        egui::CentralPanel::no_frame().show(ui, |ui| self.chat(ui, tokens));
        ready
    }

    /// Reports a successfully applied plan.
    pub(crate) fn finish(&mut self, reply: String, applied: usize) {
        self.busy = false;
        if !reply.trim().is_empty() {
            self.messages.push(ChatEntry::assistant(reply));
        }
        self.messages.push(ChatEntry::status(format!(
            "{applied} azioni applicate · Ctrl+Z per annullare"
        )));
        self.error = None;
    }

    /// Reports a plan that could not be applied.
    pub(crate) fn fail(&mut self, reply: String, error: String) {
        self.busy = false;
        if !reply.trim().is_empty() {
            self.messages.push(ChatEntry::assistant(reply));
        }
        self.messages.push(ChatEntry::error(error.clone()));
        self.error = Some(error);
    }

    fn preset(&self) -> &'static ProviderPreset {
        &PRESETS[self.provider_index]
    }

    fn apply_preset(&mut self) {
        let preset = self.preset();
        self.base_url = preset.base_url.to_owned();
        self.model = preset.model.to_owned();
        self.api_key_input.clear();
        self.key_status = key_source(preset);
        self.error = None;
    }

    fn header(&mut self, ui: &mut egui::Ui, tokens: &Tokens) {
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Assistente AI")
                    .strong()
                    .size(style::FONT_TITLE)
                    .color(tokens.text),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        !self.busy,
                        egui::Button::new(
                            RichText::new("Nuova chat")
                                .size(style::FONT_SMALL)
                                .color(tokens.text_muted),
                        )
                        .fill(tokens.surface_muted)
                        .stroke(Stroke::new(1.0, tokens.border)),
                    )
                    .clicked()
                {
                    self.reset_chat();
                }
            });
        });
        ui.label(
            RichText::new("Crea e riordina la mappa con il linguaggio naturale.")
                .size(style::FONT_SMALL)
                .color(tokens.text_muted),
        );
        ui.add_space(style::SPACE_3);
    }

    fn settings(&mut self, ui: &mut egui::Ui, tokens: &Tokens) {
        let needs_key = self.preset().needs_key && self.key_status == KeySource::Missing;
        egui::Frame::default()
            .fill(tokens.surface_muted)
            .corner_radius(egui::CornerRadius::same(style::RADIUS_CONTROL))
            .inner_margin(Margin::symmetric(10, 6))
            .show(ui, |ui| {
                egui::CollapsingHeader::new(
                    RichText::new("Provider e modello")
                        .size(style::FONT_SMALL)
                        .color(tokens.text_muted),
                )
                .id_salt("ai_settings")
                .default_open(needs_key)
                .show(ui, |ui| {
                    let previous = self.provider_index;
                    egui::ComboBox::from_id_salt("ai_provider")
                        .selected_text(self.preset().name)
                        .width(ui.available_width())
                        .show_ui(ui, |ui| {
                            for (index, preset) in PRESETS.iter().enumerate() {
                                ui.selectable_value(&mut self.provider_index, index, preset.name);
                            }
                        });
                    if self.provider_index != previous {
                        self.apply_preset();
                    }

                    ui.label(
                        RichText::new(self.preset().note)
                            .size(style::FONT_SMALL)
                            .color(tokens.text_faint),
                    );
                    ui.add_space(style::SPACE_1);

                    field_label(ui, tokens, "Modello");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.model)
                            .desired_width(f32::INFINITY)
                            .hint_text("model id"),
                    );

                    field_label(ui, tokens, "Base URL");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.base_url)
                            .desired_width(f32::INFINITY)
                            .hint_text("https://provider.example/v1"),
                    );

                    field_label(ui, tokens, "API key");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.api_key_input)
                                .password(true)
                                .desired_width(150.0)
                                .hint_text("incolla la chiave"),
                        );
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Salva").size(style::FONT_SMALL),
                                )
                                .fill(tokens.surface)
                                .stroke(Stroke::new(1.0, tokens.border)),
                            )
                            .clicked()
                        {
                            self.store_key();
                        }
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Rimuovi").size(style::FONT_SMALL),
                                )
                                .fill(tokens.surface)
                                .stroke(Stroke::new(1.0, tokens.border)),
                            )
                            .clicked()
                        {
                            self.remove_key();
                        }
                    });

                    let preset = self.preset();
                    let (text, color) = if !self.api_key_input.trim().is_empty()
                        && self.key_status == KeySource::Missing
                    {
                        (
                            "Chiave inserita per questa sessione (non ancora salvata).".to_owned(),
                            tokens.text_muted,
                        )
                    } else {
                        match self.key_status {
                            KeySource::Keyring => (
                                "Chiave salvata nel keyring di sistema.".to_owned(),
                                tokens.text_muted,
                            ),
                            KeySource::Environment => (
                                format!("Chiave letta da {}.", preset.key_env),
                                tokens.text_muted,
                            ),
                            KeySource::Missing if preset.needs_key => (
                                format!(
                                    "Nessuna chiave: inseriscila o imposta {}.",
                                    preset.key_env
                                ),
                                tokens.warning,
                            ),
                            KeySource::Missing => (
                                "Nessuna chiave necessaria per questo provider.".to_owned(),
                                tokens.text_muted,
                            ),
                        }
                    };
                    ui.label(RichText::new(text).size(style::FONT_SMALL).color(color));

                    ui.label(
                        RichText::new(
                            "La mappa corrente viene inviata al provider scelto. Le chiavi non vengono scritte nei file .mappa.",
                        )
                        .size(style::FONT_SMALL)
                        .color(tokens.text_faint),
                    );
                });
            });
    }

    fn store_key(&mut self) {
        let preset = self.preset();
        match save_api_key(preset, &self.api_key_input) {
            Ok(()) => {
                self.api_key_input.clear();
                self.key_status = KeySource::Keyring;
                self.error = None;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn remove_key(&mut self) {
        let preset = self.preset();
        match clear_api_key(preset) {
            Ok(()) => {
                self.api_key_input.clear();
                self.key_status = key_source(preset);
                self.error = None;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn chat(&mut self, ui: &mut egui::Ui, tokens: &Tokens) {
        let height = ui.available_height().max(120.0);
        egui::ScrollArea::vertical()
            .max_height(height)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for entry in &self.messages {
                    bubble(ui, entry, tokens);
                    ui.add_space(style::SPACE_2);
                }

                if self.busy {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(
                            RichText::new("In attesa del provider…")
                                .size(style::FONT_SMALL)
                                .color(tokens.text_muted),
                        );
                    });
                }
            });
    }

    fn composer(&mut self, ui: &mut egui::Ui, map: &MindMap, tokens: &Tokens) {
        ui.add_space(style::SPACE_2);
        if let Some(error) = &self.error {
            ui.label(
                RichText::new(error)
                    .size(style::FONT_SMALL)
                    .color(tokens.error),
            );
        }

        let input = egui::Frame::default()
            .fill(tokens.surface_muted)
            .stroke(Stroke::new(1.0, tokens.border))
            .corner_radius(egui::CornerRadius::same(style::RADIUS_CONTROL))
            .inner_margin(Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.input)
                        .frame(egui::Frame::NONE)
                        .margin(0)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY)
                        .hint_text("Chiedi una modifica alla mappa… (Ctrl+Invio per inviare)"),
                )
            });
        if input.inner.has_focus() {
            ui.painter().rect_stroke(
                input.response.rect.expand(1.0),
                egui::CornerRadius::same(style::RADIUS_CONTROL),
                Stroke::new(1.2, tokens.accent),
                StrokeKind::Outside,
            );
        }

        let mut send = false;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !self.busy && !self.input.trim().is_empty(),
                    egui::Button::new(
                        RichText::new("Invia")
                            .size(style::FONT_BUTTON)
                            .color(tokens.accent_text),
                    )
                    .fill(tokens.accent)
                    .stroke(Stroke::NONE),
                )
                .clicked()
            {
                send = true;
            }
            if self.busy {
                ui.spinner();
            }
            if let Some((input, output)) = self.usage {
                ui.label(
                    RichText::new(format!("token: {input} in / {output} out"))
                        .size(style::FONT_SMALL)
                        .color(tokens.text_faint),
                );
            }
        });

        let shortcut = input.inner.has_focus()
            && ui.input(|input| input.modifiers.command && input.key_pressed(Key::Enter));
        if !self.busy && (send || shortcut) && !self.input.trim().is_empty() {
            self.send(ui.ctx(), map);
        }
    }

    fn send(&mut self, ctx: &egui::Context, map: &MindMap) {
        if self.busy {
            return;
        }
        let user_text = self.input.trim().to_owned();
        if user_text.is_empty() {
            return;
        }

        let preset = self.preset();
        let api_key = if self.api_key_input.trim().is_empty() {
            load_api_key(preset)
        } else {
            Some(self.api_key_input.trim().to_owned())
        };
        let settings = ProviderSettings {
            id: preset.id.to_owned(),
            name: preset.name.to_owned(),
            protocol: preset.protocol,
            base_url: self.base_url.clone(),
            model: self.model.clone(),
            api_key,
            key_env: preset.key_env.to_owned(),
            needs_key: preset.needs_key,
        };
        let system = match system_prompt(map) {
            Ok(system) => system,
            Err(error) => {
                self.set_error(error);
                return;
            }
        };
        let mut messages = self.history();
        if let Some(last) = messages.last_mut()
            && last.role == Role::User
        {
            last.content.push_str("\n\n");
            last.content.push_str(&user_text);
        } else {
            messages.push(ChatMessage::user(user_text.clone()));
        }
        let request = ChatRequest {
            settings,
            system,
            messages,
        };

        self.messages.push(ChatEntry::user(user_text));
        self.input.clear();
        self.busy = true;
        self.error = None;
        self.usage = None;

        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        thread::spawn(move || {
            let result = ProviderClient::new().and_then(|client| client.send(&request));
            let _ = sender.send(WorkerEvent::Finished(result));
        });
        ctx.request_repaint();
    }

    fn history(&self) -> Vec<ChatMessage> {
        let mut messages: Vec<ChatMessage> = self
            .messages
            .iter()
            .filter_map(|entry| match entry.kind {
                EntryKind::User => Some(ChatMessage::user(entry.text.clone())),
                EntryKind::Assistant => Some(ChatMessage::assistant(entry.text.clone())),
                EntryKind::Status | EntryKind::Error => None,
            })
            .collect();
        if messages.len() > 12 {
            messages.drain(..messages.len() - 12);
        }
        // Anthropic expects the conversation to start with a user turn and to
        // alternate roles. Merge consecutive same-role turns and drop a leading
        // assistant turn left over after trimming.
        while messages
            .first()
            .is_some_and(|message| message.role == Role::Assistant)
        {
            messages.remove(0);
        }
        let mut normalized: Vec<ChatMessage> = Vec::with_capacity(messages.len());
        for message in messages {
            if let Some(last) = normalized.last_mut()
                && last.role == message.role
            {
                last.content.push_str("\n\n");
                last.content.push_str(&message.content);
            } else {
                normalized.push(message);
            }
        }
        normalized
    }

    fn poll(&mut self, ctx: &egui::Context) -> Option<AiPanelAction> {
        let state = {
            let Some(receiver) = &self.receiver else {
                return None;
            };
            match receiver.try_recv() {
                Ok(event) => PollState::Ready(event),
                Err(TryRecvError::Empty) => PollState::Empty,
                Err(TryRecvError::Disconnected) => PollState::Disconnected,
            }
        };
        let event = match state {
            PollState::Ready(event) => event,
            PollState::Empty => return None,
            PollState::Disconnected => {
                self.receiver = None;
                self.busy = false;
                self.set_error(AiError::Network(
                    "the AI worker stopped without a response".to_owned(),
                ));
                return None;
            }
        };

        self.receiver = None;
        self.busy = false;
        let WorkerEvent::Finished(result) = event;
        match result {
            Ok(response) => {
                self.usage = match (response.usage.input, response.usage.output) {
                    (None, None) => None,
                    (input, output) => Some((input.unwrap_or(0), output.unwrap_or(0))),
                };
                match parse_plan(&response.text) {
                    Ok(plan) if plan.actions.is_empty() => {
                        let reply = plan.reply.trim();
                        self.messages
                            .push(ChatEntry::assistant(if reply.is_empty() {
                                "(nessuna modifica)"
                            } else {
                                reply
                            }));
                        self.messages
                            .push(ChatEntry::status("Nessuna modifica alla mappa."));
                        self.error = None;
                    }
                    Ok(plan) => {
                        return Some(AiPanelAction::Apply {
                            reply: plan.reply.clone(),
                            plan,
                        });
                    }
                    Err(error) => {
                        let raw = response.text.trim();
                        if !raw.is_empty() {
                            self.messages.push(ChatEntry::assistant(raw.to_owned()));
                        }
                        self.set_error(error);
                    }
                }
            }
            Err(error) => self.set_error(error),
        }
        ctx.request_repaint();
        None
    }

    fn set_error(&mut self, error: AiError) {
        let message = error.to_string();
        self.error = Some(message.clone());
        self.messages.push(ChatEntry::error(message));
    }

    fn reset_chat(&mut self) {
        if self.busy {
            return;
        }
        self.messages.clear();
        self.messages.push(ChatEntry::status(WELCOME));
        self.usage = None;
        self.error = None;
    }
}

fn field_label(ui: &mut egui::Ui, tokens: &Tokens, text: &str) {
    ui.add_space(style::SPACE_1);
    ui.label(
        RichText::new(text)
            .size(style::FONT_SMALL)
            .color(tokens.text_muted),
    );
}

fn bubble(ui: &mut egui::Ui, entry: &ChatEntry, tokens: &Tokens) {
    let max_width = (ui.available_width() * 0.84).max(160.0);
    let corner = egui::CornerRadius::same(style::RADIUS_PANEL);
    let margin = Margin::symmetric(10, 8);
    match entry.kind {
        EntryKind::Status => {
            ui.label(
                RichText::new(&entry.text)
                    .italics()
                    .size(style::FONT_SMALL)
                    .color(tokens.text_muted),
            );
        }
        EntryKind::Error => {
            egui::Frame::default()
                .fill(style::with_alpha(tokens.error, 22))
                .stroke(Stroke::new(1.0, style::with_alpha(tokens.error, 90)))
                .corner_radius(corner)
                .inner_margin(margin)
                .show(ui, |ui| {
                    ui.set_max_width(max_width);
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("Errore: {}", entry.text))
                                .size(style::FONT_BUTTON)
                                .color(tokens.error),
                        )
                        .wrap(),
                    );
                });
        }
        EntryKind::User => {
            ui.with_layout(egui::Layout::right_to_left(Align::Min), |ui| {
                egui::Frame::default()
                    .fill(tokens.accent_soft)
                    .corner_radius(corner)
                    .inner_margin(margin)
                    .show(ui, |ui| {
                        ui.with_layout(egui::Layout::top_down(Align::Min), |ui| {
                            ui.set_max_width(max_width);
                            ui.label(
                                RichText::new("Tu")
                                    .strong()
                                    .size(style::FONT_SMALL)
                                    .color(tokens.accent),
                            );
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&entry.text)
                                        .size(style::FONT_BUTTON)
                                        .color(tokens.text),
                                )
                                .wrap(),
                            );
                        });
                    });
            });
        }
        EntryKind::Assistant => {
            egui::Frame::default()
                .fill(tokens.surface)
                .stroke(Stroke::new(1.0, tokens.border))
                .corner_radius(corner)
                .inner_margin(margin)
                .show(ui, |ui| {
                    ui.set_max_width(max_width);
                    ui.label(
                        RichText::new("AI")
                            .strong()
                            .size(style::FONT_SMALL)
                            .color(tokens.text_muted),
                    );
                    ui.add(
                        egui::Label::new(
                            RichText::new(&entry.text)
                                .size(style::FONT_BUTTON)
                                .color(tokens.text),
                        )
                        .wrap(),
                    );
                });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::{Duration, Instant};

    use serde_json::{Value, json};

    use super::*;
    use mappamente_ai::{PRESETS, apply_actions};

    fn mock_plan_server(content: String) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let mut length = 0usize;
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                if header == "\r\n" || header.is_empty() {
                    break;
                }
                if let Some((name, value)) = header.split_once(':')
                    && name.eq_ignore_ascii_case("content-length")
                {
                    length = value.trim().parse().unwrap();
                }
            }
            let mut body = vec![0u8; length];
            reader.read_exact(&mut body).unwrap();
            let _request: Value = serde_json::from_slice(&body).unwrap();

            let response = json!({
                "choices": [{"message": {"content": content}}]
            })
            .to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
                response.len()
            )
            .unwrap();
            stream.flush().unwrap();
        });
        (format!("http://{address}"), handle)
    }

    #[test]
    fn history_starts_with_a_user_turn_and_merges_same_roles() {
        let panel = AiPanel {
            provider_index: 0,
            base_url: String::new(),
            model: String::new(),
            api_key_input: String::new(),
            key_status: KeySource::Missing,
            initialized: true,
            messages: vec![
                ChatEntry::assistant("welcome"),
                ChatEntry::user("first"),
                ChatEntry::user("second"),
                ChatEntry::assistant("reply"),
            ],
            input: String::new(),
            busy: false,
            error: None,
            receiver: None,
            usage: None,
        };
        let history = panel.history();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].role, Role::User);
        assert_eq!(history[0].content, "first\n\nsecond");
        assert_eq!(history[1].role, Role::Assistant);
    }

    #[test]
    fn worker_response_is_parsed_and_can_update_the_map() {
        let mut map = MindMap::new("Test", "Root");
        let root = map.root.id.clone();
        let plan = json!({
            "reply": "Fatto",
            "actions": [
                {"op": "add_node", "parent_id": root, "text": "From AI"}
            ]
        });
        let (base, server) = mock_plan_server(plan.to_string());

        let index = PRESETS
            .iter()
            .position(|preset| preset.id == "custom")
            .unwrap();
        let mut panel = AiPanel {
            provider_index: index,
            base_url: format!("{base}/v1"),
            model: "test-model".to_owned(),
            api_key_input: String::new(),
            key_status: KeySource::Missing,
            initialized: true,
            messages: Vec::new(),
            input: "Crea un nodo".to_owned(),
            busy: false,
            error: None,
            receiver: None,
            usage: None,
        };
        let context = egui::Context::default();
        panel.send(&context, &map);

        let deadline = Instant::now() + Duration::from_secs(5);
        let action = loop {
            if let Some(action) = panel.poll(&context) {
                break action;
            }
            assert!(Instant::now() < deadline, "AI worker timed out");
            thread::sleep(Duration::from_millis(20));
        };

        match action {
            AiPanelAction::Apply { reply, plan } => {
                assert_eq!(reply, "Fatto");
                let summary = map
                    .edit(|draft| apply_actions(draft, &plan.actions))
                    .unwrap();
                assert_eq!(summary.applied, 1);
                assert!(
                    map.find(&root)
                        .unwrap()
                        .children
                        .iter()
                        .any(|node| node.text == "From AI")
                );
            }
        }
        server.join().unwrap();
    }
}
