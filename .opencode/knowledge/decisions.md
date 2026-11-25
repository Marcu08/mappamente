# Decisions

## D1 — GUI with egui/eframe, not Tauri

**Date:** 2026-11-25
**Status:** accepted

Requirement: lightweight and open source. `eframe` produces native binaries
with no WebView or Node toolchain and starts in milliseconds. The canvas is
custom-painted anyway, so the web UI advantage of Tauri is small. Trade-off:
less polished native text editing; acceptable for node labels.

## D2 — Native format is JSON, XML only for interop

**Date:** 2026-11-25
**Status:** accepted

JSON maps directly to serde, has no schema boilerplate, and keeps the file
compact. XML is not the internal model; OPML 2.0 and FreeMind import/export are
planned because those ecosystems use it. Markdown outlines are implemented for
AI/human interchange. Version field and unknown-field tolerance handle
evolution.

## D3 — MCP server and in-app AI share one tool layer

**Date:** 2026-11-25
**Status:** accepted

The same tool definitions and handlers will serve:
1. the stdio MCP server for external clients (Claude Desktop, Cursor, ...);
2. the in-app AI panel that accepts an API key.

This avoids duplicated validation and keeps behavior identical. External MCP
writes files atomically; the GUI watches and reloads them for live feedback.
An optional loopback HTTP transport can be added later if file races become a
problem.

## D4 — Snapshot undo with coalescing

**Date:** 2026-11-25
**Status:** accepted

Maps are small, so snapshotting the whole document per transaction is simpler
and safer than inverse commands. Continuous interactions use
`apply_coalesced` with a label and time window so a drag is one undo step.
Discrete `apply` calls never coalesce. AI generation will be a single
transaction by construction.

## D5 — Single binary

**Date:** 2026-11-25
**Status:** superseded by D8

The original plan was one executable with GUI, CLI and MCP modes. Windows makes
this awkward: a console-subsystem binary flashes a console when double-clicked,
while a GUI-subsystem binary hides CLI output. D8 replaces this decision.

## D6 — API keys in the OS credential store

**Date:** 2026-11-25
**Status:** accepted

Use `keyring` (Windows Credential Manager, macOS Keychain, Secret Service) with
an environment-variable fallback for CI/advanced users. Keys are never written
to `.mappa` files or logs. Provider base URL is configurable, enabling
OpenRouter, Groq, DeepSeek, Ollama and LM Studio.

## D7 — Naming

**Date:** 2026-11-25
**Status:** accepted

Project and console binary: `mappamente`. Desktop binary: `mappamente-desktop`.
Library: `mappamente-core`. Native extension `.mappa`. Media type (proposed):
`application/vnd.mappa+json`.

## D8 — Two binaries: console CLI and GUI-subsystem launcher

**Date:** 2026-11-25
**Status:** accepted

`mappamente.exe` stays a console binary so CLI output and the future stdio MCP
server work correctly. `mappamente-desktop.exe` is built with
`windows_subsystem = "windows"` on Windows release builds and is the target of
the Desktop shortcut. Both share the same library. Cost: two artifacts and
roughly twice the executable size; benefit: no console flash and intact CLI.

## D9 — Markdown outline is a strict single-root format

**Date:** 2026-11-25
**Status:** accepted

Markdown import requires exactly one root bullet; deeper indentation creates
children. A leading heading becomes the title, otherwise the root label does.
This is unambiguous, easy for LLMs to produce, and catches malformed output
early. Notes, colors, positions and collapsed state are intentionally lost;
native JSON is the lossless format.

## D10 — SVG export shares layout and palette with the GUI

**Date:** 2026-11-25
**Status:** accepted

The renderer consumes `mappamente_core::layout` and `mappamente_core::theme`, so exported
maps match the canvas instead of using a second geometry implementation. Text
metrics are estimated in core; the SVG font may wrap a few pixels differently
from the GUI.

## D11 — Read-only preview before the editor

**Date:** 2026-11-25
**Status:** completed, superseded by D12

Milestone 2 needed a real window so the Desktop shortcut would be meaningful
and so layout/theme choices could be validated visually. The preview opened
maps, panned/zoomed, switched layout/theme, accepted drops and showed native
dialogs without mutating files. Milestone 3 replaced it with the editing canvas.

## D12 — Editing interaction model

**Date:** 2026-11-25
**Status:** accepted

- Double-click or `F2` edits inline; `Tab` adds a child, `Enter` inserts a
  sibling after the selection, `Delete` removes a branch; root is protected.
- Dragging a node applies coalesced manual positions for live feedback. On
  release the drop target is searched **under the dragged branch** (the moving
  node covers it), then `move_node` + `set_pos(None)` reparents.
- Every change goes through `Editor`, so undo, dirty tracking and transactions
  are shared with the core.
- Pending text is committed before New/Open/Save/Close, and before layout/theme
  changes, to avoid losing the inline buffer.

## D13 — Layout and theme are document properties

**Date:** 2026-11-25
**Status:** accepted

Switching layout or theme applies a document edit, persists on save and is
undoable. `after_change` synchronizes the view state from the map so undo/redo
restores the correct layout/theme. This avoids the surprising case where the
user changes the layout, saves, reopens, and sees the old one.

## D14 — Brand: Mappamente

**Date:** 2026-11-25
**Status:** accepted

The name combines *mappa* (map) and *mente* (mind); it is explicitly about mind
maps and was free on the owner's GitHub account. The logo is an “M” drawn as a
route between nodes on a dark rounded square. The native extension remains the
short `.mappa`; the console binary is `mappamente`, the GUI-subsystem binary is
`mappamente-desktop`, and the repository is `Marcu08/mappamente`.

