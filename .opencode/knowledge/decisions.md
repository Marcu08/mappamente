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
**Status:** accepted in principle; not fully realized yet (see D18)

The same tool definitions and handlers will serve:
1. the stdio MCP server for external clients (Claude Desktop, Cursor, ...);
2. the in-app AI panel that accepts an API key.

This avoids duplicated validation and keeps behavior identical. External MCP
writes files atomically; the GUI watches and reloads them for live feedback.
An optional loopback HTTP transport can be added later if file races become a
problem. The M3 implementation uses a separate in-memory JSON action protocol
for the AI panel; unifying it with the MCP registry is tracked as follow-up
work.

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
**Status:** accepted; implemented for the AI panel in M3

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

## D15 — MCP server is a separate stdio binary with a sandboxed directory

**Date:** 2026-11-25
**Status:** accepted

`mappamente-mcp` is its own crate and binary instead of a `mappamente mcp`
subcommand. Reasons: the CLI and GUI do not need async/tokio or rmcp at
runtime; the MCP binary can be distributed and versioned independently; and a
stdio process has a clean stdout channel. It uses `rmcp` 3.11 with the
`macros`, `server` and `transport-io` features and tokio 1.54.

`--dir` is mandatory and must exist. Every map name must be a single file name
with `.mappa`/`.json`; existing paths are canonicalized and must remain under
the canonical root. This blocks traversal, absolute paths, Windows device names
and symlinks that point outside the directory. Mutations load the map, apply
`MindMap::edit`, and write through the core atomic writer. `modified_at` is a
strictly increasing token; `expected_modified_at` is optional (omitted means
last write wins).

Domain failures are returned as tool results with `isError: true` and a stable
`code`, never as protocol errors or panics. Limits: 10,000 characters for
labels/notes, 100,000 for Markdown input, 5 MiB for map files. Link tools are
deliberately not exposed yet. Tool-level export writes only inside `--dir` and
requires `overwrite: true` for existing files; native export refuses to replace
its own source.

## D16 — No `panic = "abort"` in the release profile

**Date:** 2026-11-25
**Status:** accepted

The release profile previously aborted on panic, which saved little binary size
and made the long-running MCP server die without unwinding. The MCP server and
future in-app tool loop run inside async tasks, where a panic should be reported
as a task failure rather than terminating the process. Tests also rely on
normal unwinding. The core still forbids unsafe code and avoids panics outside
tests, so this changes failure containment, not normal behavior.

## D17 — Markdown subtree insertion drops the parsed outline root

**Date:** 2026-11-25
**Status:** accepted

`from_markdown` always creates one root bullet because a standalone Markdown
document needs a root. When an agent inserts an outline below an existing node,
that extra root is usually a heading, so `insert_markdown_children` discards it
when it has children and inserts the children in order. A single-line outline
has no children, so its root is inserted as the new node instead of losing the
content. A leading `# Title` becomes the temporary map title and is never
inserted. All ids are regenerated by `insert_subtree`.

## D18 - AI panel uses a JSON action protocol, not provider-native tools

**Date:** 2026-11-25
**Status:** accepted

OpenAI-compatible providers, Anthropic and the local OpenCode server expose
different tool-calling shapes. Requiring the model to return one JSON object
with a `reply` and an `actions` array keeps all providers on one path and avoids
giving the assistant arbitrary filesystem or shell tools. `mappamente-ai`
parses the plan and the app applies it inside one `Editor::apply` transaction,
so the canvas updates immediately, Ctrl+Z undoes the whole turn, and a bad
action rolls everything back.

## D19 - Central style tokens, branch palette and user-local UI preferences

**Date:** 2026-11-25
**Status:** accepted

Visual constants live in `crates/mappamente/src/style.rs`; components consume
semantic tokens instead of raw colors. The four existing document palettes are
mapped to neutral surfaces plus one accent, and `theme.rs` now exposes eight
branch hues used by both the canvas and SVG export. Explicit `node.color`
always wins; text uses a contrast-based white/near-black choice.

The editor chrome is intentionally light: a floating icon toolbar, a status
pill over the canvas, adaptive grid spacing and about 180 ms transitions. There
is no reduce-motion OS API in egui 0.37, so motion is a user preference. Theme,
grid and motion preferences persist in the platform config directory
(`ui.conf`), never next to `.mappa` files. New/welcome maps follow the OS theme
first; an opened map keeps its own document theme.

No runtime dependency was added for fonts, icons or animation. System fonts
are provided by egui/eframe, and toolbar icons are painted with the egui
painter.

Trade-offs: models can return malformed JSON (shown as an error, no panic); the
protocol has no automatic retry; and MCP tools and AI actions are still
separate implementations. A shared registry remains the target from D3.

## D19 — AI providers: presets, keyring and explicit network policy

**Date:** 2026-11-25
**Status:** accepted

The panel ships presets for OpenAI, Anthropic, OpenCode Console, OpenCode local,
OpenRouter, Groq, DeepSeek, Mistral, Google Gemini, xAI, Ollama, LM Studio and a
custom OpenAI-compatible endpoint. Users can edit model and base URL. OpenCode
Console uses its OpenAI-compatible gateway; a local `opencode serve` uses its
one-shot `/api/experimental/generate/text` endpoint and optional Basic auth.

Keys are stored in the OS keyring and fall back to per-provider environment
variables. Remote endpoints must use HTTPS; HTTP is allowed only for literal
loopback hosts. Requests carry a 120,000-character map-outline limit, the client
has a 120-second timeout and follows no redirects, and error bodies are
truncated before display. The API key is never logged.

## D20 — AI requests run on a plain worker thread

**Date:** 2026-11-25
**Status:** accepted

The eframe UI is synchronous, so the AI panel uses `reqwest::blocking` on a
`std::thread` and reports results through an `mpsc` channel. This avoids adding a
Tokio runtime to the GUI event loop and keeps the canvas responsive. Only one
request runs at a time; there is no cancel or token streaming yet. A result is
parsed, applied, and shown after the worker finishes.

