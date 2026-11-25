# Project context

**Mappamente** is a lightweight, open-source mind map editor written in Rust.
The name combines *mappa* (map) and *mente* (mind). It ships as a native window
plus a console CLI, with plain-text files, an optional AI panel using the user's
own API key, and an MCP server so external agents can create and edit maps.

Repository: <https://github.com/Marcu08/mappamente>

## Non-negotiables

- Lightweight native app: `egui`/`eframe`, no WebView, no Electron.
- Plain-text, user-owned files: native `.mappa` JSON, atomic writes.
- Offline by default. Network access only for the AI provider explicitly chosen
  by the user; no telemetry.
- API keys stored in the OS credential store (Windows Credential Manager,
  macOS Keychain, Secret Service), never in map files or logs.
- MCP and the in-app AI must use one shared tool layer, not two implementations.
- MIT OR Apache-2.0.

## Stack

- Rust edition 2024, workspace resolver 3.
- Core: `serde`, `serde_json`, `thiserror`, `fastrand`, `unicode-width`.
- GUI: `eframe`/`egui` 0.37.
- CLI: `clap` 4.6 derive, `anyhow`.
- Dialogs: `rfd` 0.18 (xdg-portal on Linux).
- MCP: `rmcp` 3.11 (`macros`, `server`, `transport-io`), `tokio` 1.54,
  `schemars` 1.2 over stdio.
- AI: `reqwest` 0.13 blocking with rustls, `keyring` 4.2, `mappamente-ai` crate.
- Planned AI: `reqwest` (rustls), `keyring`, OpenAI-compatible + Anthropic
  adapters.

## Layout of the repository

```
Cargo.toml                     workspace
crates/mappamente-core/        headless core (implemented)
crates/mappamente/             lib + mappamente.exe (CLI) + mappamente-desktop.exe
crates/mappamente-mcp/         stdio MCP server (mappamente-mcp.exe)
crates/mappamente-ai/          AI providers, credentials and JSON map actions
assets/                        icon.png, mappamente.ico
scripts/install-shortcut.ps1   Desktop/Start Menu shortcut
docs/                          demo map, screenshots, format and CLI docs
.github/workflows/ci.yml       fmt, clippy -D warnings, tests on 3 OSes
```

## Status (2026-11-25) — v0.1.0 plus unreleased work

Milestones 1–3, the external-use changes, the MCP server and the AI panel are
complete in the working tree:

- core model, JSON, layouts, undo/redo, validation, tests;
- Markdown outline import/export, SVG export with four themes;
- CLI `new`, `convert`, `render`, `gui`;
- native editing canvas: inline text, add/delete, sibling ordering, drag to
  move/reparent, undo/redo, layout/theme switching persisted, open/save/save-as,
  drag & drop, unsaved-change guard, custom window icon;
- Desktop shortcut `Mappamente.lnk` created via `scripts/install-shortcut.ps1`;
- transactional `MindMap::edit`, `outline_with_ids`, Markdown subtree insertion,
  JSON migration hook, 5 MiB map-file limit;
- `mappamente-mcp`: stdio MCP server with ten tools, sandboxed `--dir`,
  optimistic concurrency and tool-level errors;
- `mappamente-ai` + `ai_panel.rs`: provider presets (OpenAI, Anthropic,
  OpenCode Console/local, OpenRouter, Groq, DeepSeek, Mistral, Gemini, xAI,
  Ollama, LM Studio, custom), OS-keyring keys with env fallback, JSON action
  protocol and live canvas updates in one undo step;
- 146 tests, CI for three operating systems, dual license.

Known architecture debt: MCP file tools and AI actions are still separate; the
shared tool registry from D3 is pending.

Next: MCP live reload, then OPML/FreeMind, autosave and a shared tool registry.

## Commands

```bash
cargo test --all
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
cargo build --release
```

MCP smoke run:

```powershell
cargo run -p mappamente-mcp -- --dir .\docs
```

```powershell
powershell -ExecutionPolicy Bypass -File scripts/install-shortcut.ps1
```

This workspace's environment has an empty `CARGO_TARGET_DIR`, which makes cargo
fail. Set it explicitly when running commands in this session:

```powershell
$env:CARGO_TARGET_DIR = "<workspace>\target"
```

For Windows UI smoke tests, make the PowerShell thread DPI-aware before calling
`SetCursorPos`, otherwise clicks are scaled and miss node rectangles:

```powershell
[void][User32]::SetThreadDpiAwarenessContext([IntPtr](-4))
```
