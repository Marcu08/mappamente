# Changelog

All notable changes to Mappamente are documented here.

## [Unreleased]

### Added

- Core: transactional `MindMap::edit`, `outline_with_ids`, Markdown subtree
  insertion with fresh ids, JSON migration hook and a 5 MiB map-file limit.
- `mappamente-mcp`: stdio MCP server with sandboxed `--dir`, ten map tools,
  optimistic concurrency via `modified_at`/`expected_modified_at`, atomic
  writes and tool-level errors.
- AI assistant side panel with OpenAI, Anthropic, OpenCode Console/local,
  OpenRouter, Groq, DeepSeek, Mistral, Gemini, xAI, Ollama, LM Studio and custom
  OpenAI-compatible providers. Keys use the OS keyring with environment
  fallbacks; returned JSON actions are applied to the live canvas in one undo
  step.
- Visual redesign: semantic style tokens, a floating vector-icon toolbar with
  symmetric 24×24 glyphs, pill nodes with a curated accent and eight branch
  hues, branch-coloured Bézier connectors, an adaptive dot grid, animated
  selection/hover/fit, a minimal status line and a restyled AI panel with
  collapsed provider settings.
- User-local preferences for theme, grid and motion in the platform config
  directory; new/welcome maps follow the OS theme by default.
- Optional portable install: `install-shortcut.ps1 -Destination <folder>
  -Portable` copies `Mappamente.exe`, embeds the application icon into it and
  creates an icon shortcut next to it.
- SVG export now shares the branch palette, readable text colors and pill
  geometry with the canvas.
- README, design notes, file-format, architecture and decision documentation
  updates.

### Changed

- Release profile no longer aborts on panic, so the long-running MCP server and
  future tool loops can unwind.
- 155 tests, including local mock HTTP coverage for all three AI protocols.

### Visual changes

- Toolbar icons are drawn on a 24×24 logical grid so every glyph is symmetric,
  straight and balanced; the icon canvas is larger and the stroke is slightly
  heavier for better readability at toolbar size.  Save, Save As and the AI
  assistant icons were refined to be immediately readable.
- Node labels are vertically centered using real font metrics instead of a fixed
  line height, so single-line and wrapped text both sit cleanly inside pills.
- Trackpad pinch-to-zoom now zooms the canvas in addition to the scroll-wheel
  zoom; both zoom toward the pointer position.
- The window title dirty marker is unchanged; the bottom status line now shows
  only file state, zoom and node count.
- Layout and theme remain document properties; switching them still participates
  in undo and is saved with the map.
- SVG output changed consciously: node fills, branch-colored edges, text
  contrast and radii now match the redesigned canvas. The `.mappa` schema is
  unchanged.

## [0.1.0] - 2026-11-25

First public version.

### Added

- Native editing canvas built with `egui`/`eframe`:
  - inline node text editing with `F2` or double-click;
  - `Tab` adds a child, `Enter` inserts a sibling, `Delete` removes a branch;
  - drag to move a node or reparent it onto another node;
  - pan, zoom and fit-to-view;
  - layout (`tree`, `radial`, `logical`) and theme (`dark`, `light`, `paper`,
    `ocean`) switching, persisted in the document;
  - undo/redo with transactions and coalesced drags;
  - open/save/save-as, drag & drop, unsaved-change guard and window icon.
- Headless CLI: `new`, `convert`, `render`, `gui`.
- Markdown outline import/export.
- Standalone SVG export with shared layout, palettes, shadows and wrapped text.
- Core model with short stable IDs, atomic JSON persistence, schema validation
  and forward-compatible loading.
- Deterministic layouts with overlap guarantees, tested on 1,111 nodes.
- Desktop/Start Menu shortcut installer with a custom icon.
- CI for Linux, Windows and macOS; 88 tests; fmt, clippy and rustdoc checks.

### Known limitations

- No autosave, notes/colors editing UI or collapse toggle in the canvas yet.
- OPML, FreeMind, PNG and PDF are not implemented.
- MCP server and AI panel are planned for later milestones.
- Windows binaries are the only pre-built release artifacts; other platforms
  build from source.

[0.1.0]: https://github.com/Marcu08/mappamente/releases/tag/v0.1.0
