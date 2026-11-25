//! AI assistant side panel.
//!
//! Network calls run on a worker thread; the UI polls a channel and applies a
//! parsed plan through the app's `Editor`, so the canvas updates immediately and
//! every turn is one undo step.

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use eframe::egui::{self, Color32, Key, RichText};
use mappamente_ai::{
    AiError, ChatMessage, ChatRequest, ChatResponse, KeySource, PRESETS, Plan, ProviderClient,
    ProviderPreset, ProviderSettings, Role, clear_api_key, key_source, load_api_key, parse_plan,
    save_api_key, system_prompt,
};
use mappamente_core::MindMap;

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
    pub(crate) fn ui(&mut self, ui: &mut egui::Ui, map: &MindMap) -> Option<AiPanelAction> {
        if !self.initialized {
            self.initialized = true;
            self.key_status = key_source(self.preset());
        }
        let ready = self.poll(ui.ctx());
        self.header(ui);
        self.settings(ui);
        ui.separator();
        self.chat(ui);
        self.composer(ui, map);
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

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Assistente AI");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Nuova chat"))
                    .clicked()
                {
                    self.reset_chat();
                }
            });
        });
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Provider e modello")
            .default_open(true)
            .show(ui, |ui| {
                let previous = self.provider_index;
                egui::ComboBox::from_id_salt("ai_provider")
                    .selected_text(self.preset().name)
                    .width(220.0)
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
                        .size(11.0)
                        .color(ui.visuals().weak_text_color()),
                );
                ui.add_space(4.0);

                ui.label("Modello");
                ui.add(
                    egui::TextEdit::singleline(&mut self.model)
                        .desired_width(f32::INFINITY)
                        .hint_text("model id"),
                );

                ui.label("Base URL");
                ui.add(
                    egui::TextEdit::singleline(&mut self.base_url)
                        .desired_width(f32::INFINITY)
                        .hint_text("https://provider.example/v1"),
                );

                ui.label("API key");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.api_key_input)
                            .password(true)
                            .desired_width(170.0)
                            .hint_text("incolla la chiave"),
                    );
                    if ui.button("Salva").clicked() {
                        self.store_key();
                    }
                    if ui.button("Rimuovi").clicked() {
                        self.remove_key();
                    }
                });

                let preset = self.preset();
                let (text, color) = if !self.api_key_input.trim().is_empty()
                    && self.key_status == KeySource::Missing
                {
                    (
                        "Chiave inserita per questa sessione (non ancora salvata).".to_owned(),
                        ui.visuals().weak_text_color(),
                    )
                } else {
                    match self.key_status {
                        KeySource::Keyring => (
                            "Chiave salvata nel keyring di sistema.".to_owned(),
                            ui.visuals().weak_text_color(),
                        ),
                        KeySource::Environment => (
                            format!("Chiave letta da {}.", preset.key_env),
                            ui.visuals().weak_text_color(),
                        ),
                        KeySource::Missing if preset.needs_key => (
                            format!(
                                "Nessuna chiave: inseriscila o imposta {}.",
                                preset.key_env
                            ),
                            Color32::from_rgb(210, 150, 60),
                        ),
                        KeySource::Missing => (
                            "Nessuna chiave necessaria per questo provider.".to_owned(),
                            ui.visuals().weak_text_color(),
                        ),
                    }
                };
                ui.label(RichText::new(text).size(11.0).color(color));

                ui.label(
                    RichText::new(
                        "La mappa corrente viene inviata al provider scelto. Le chiavi non vengono scritte nei file .mappa.",
                    )
                    .size(11.0)
                    .color(ui.visuals().weak_text_color()),
                );
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

    fn chat(&mut self, ui: &mut egui::Ui) {
        let height = (ui.available_height() - 115.0).max(120.0);
        egui::ScrollArea::vertical()
            .max_height(height)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for entry in &self.messages {
                    match entry.kind {
                        EntryKind::User => {
                            ui.label(
                                RichText::new("Tu")
                                    .strong()
                                    .size(11.0)
                                    .color(ui.visuals().hyperlink_color),
                            );
                            ui.label(RichText::new(&entry.text).size(13.0));
                        }
                        EntryKind::Assistant => {
                            ui.label(
                                RichText::new("AI")
                                    .strong()
                                    .size(11.0)
                                    .color(ui.visuals().strong_text_color()),
                            );
                            ui.label(RichText::new(&entry.text).size(13.0));
                        }
                        EntryKind::Status => {
                            ui.label(
                                RichText::new(&entry.text)
                                    .italics()
                                    .size(11.0)
                                    .color(ui.visuals().weak_text_color()),
                            );
                        }
                        EntryKind::Error => {
                            ui.label(
                                RichText::new(format!("Errore: {}", entry.text))
                                    .size(12.0)
                                    .color(Color32::from_rgb(210, 80, 80)),
                            );
                        }
                    }
                    ui.add_space(6.0);
                }

                if self.busy {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(
                            RichText::new("In attesa del provider…")
                                .size(12.0)
                                .color(ui.visuals().weak_text_color()),
                        );
                    });
                }
            });
    }

    fn composer(&mut self, ui: &mut egui::Ui, map: &MindMap) {
        ui.separator();
        if let Some(error) = &self.error {
            ui.label(
                RichText::new(error)
                    .size(12.0)
                    .color(Color32::from_rgb(210, 80, 80)),
            );
        }

        let response = ui.add(
            egui::TextEdit::multiline(&mut self.input)
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .hint_text("Chiedi una modifica alla mappa… (Ctrl+Invio per inviare)"),
        );

        let mut send = false;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !self.busy && !self.input.trim().is_empty(),
                    egui::Button::new("Invia"),
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
                        .size(11.0)
                        .color(ui.visuals().weak_text_color()),
                );
            }
        });

        let shortcut = response.has_focus()
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
