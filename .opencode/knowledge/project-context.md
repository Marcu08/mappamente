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
- Planned MCP: `rmcp` 3.x over stdio.
- Planned AI: `reqwest` (rustls), `keyring`, OpenAI-compatible + Anthropic
  adapters.

## Layout of the repository

```
Cargo.toml                     workspace
crates/mappamente-core/        headless core (implemented)
crates/mappamente/             lib + mappamente.exe (CLI) + mappamente-desktop.exe
assets/                        icon.png, mappamente.ico
scripts/install-shortcut.ps1   Desktop/Start Menu shortcut
docs/                          demo map, screenshots, format and CLI docs
.github/workflows/ci.yml       fmt, clippy -D warnings, tests on 3 OSes
```

## Status (2026-11-25) — v0.1.0

Milestones 1–3 complete:

- core model, JSON, layouts, undo/redo, validation, tests;
- Markdown outline import/export, SVG export with four themes;
- CLI `new`, `convert`, `render`, `gui`;
- native editing canvas: inline text, add/delete, sibling ordering, drag to
  move/reparent, undo/redo, layout/theme switching persisted, open/save/save-as,
  drag & drop, unsaved-change guard, custom window icon;
- Desktop shortcut `Mappamente.lnk` created via `scripts/install-shortcut.ps1`;
- 88 tests, CI for three operating systems, dual license.

Next: autosave and richer node metadata, then OPML/FreeMind, MCP and AI.

## Commands

```bash
cargo test --all
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
cargo build --release
```

```powershell
powershell -ExecutionPolicy Bypass -File scripts/install-shortcut.ps1
```

This workspace's environment has an empty `CARGO_TARGET_DIR`, which makes cargo
fail. Set it explicitly when running commands in this session:

```powershell
$env:CARGO_TARGET_DIR = "C:\Users\marzu\OneDrive\Desktop\app_mappe\target"
```

For Windows UI smoke tests, make the PowerShell thread DPI-aware before calling
`SetCursorPos`, otherwise clicks are scaled and miss node rectangles:

```powershell
[void][User32]::SetThreadDpiAwarenessContext([IntPtr](-4))
```
