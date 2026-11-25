# Architecture

## Workspace

### `mappamente-core` (implemented, no GUI dependencies)

- `model` — `MindMap`, `Node`, `Link`, `LayoutKind`, short id generation, tree
  operations, validation, outline rendering.
- `layout` — deterministic placement, text wrapping, metrics, manual offsets.
  `Tree` is bilateral, `Radial` spreads by leaf weight, `Logical` grows right.
  Layout depth equals document depth for all algorithms.
- `history` — `Editor`, snapshot undo/redo, transactions, drag coalescing,
  dirty tracking by monotonic revision.
- `io::json` — pretty stable JSON, validation, atomic save/load.
- `io::markdown` — title + indented bullet outline; exactly one root; lossy.
  `io::load_path`/`save_path` dispatch by extension.
- `export::svg` — standalone SVG, same layout/palettes as the GUI, escaped
  labels, shadows, wrapped text.
- `theme` — four palettes (`dark`, `light`, `paper`, `ocean`) as hex strings,
  hex parsing and contrast helpers.
- `geom` — `Point`, `Size`, `Rect`, edge anchors.

### `mappamente` (lib + two binaries)

- `cli.rs` — clap definitions and command implementations. No command opens
  the editor; otherwise `new`/`convert`/`render`/`gui`.
- `gui.rs` — native editing canvas:
  - top bar: New, Open, Save, Save as, Undo, Redo, Fit, layout, theme, file,
    node count, error banner;
  - central canvas: grid, curved edges, nodes, selection ring, inline editor;
  - bottom bar: status and shortcut hints;
  - pan/zoom/fit, drag move, drag-to-reparent, keyboard editing, save/discard
    guards.
- `bin/mappamente` — console binary (`src/main.rs`), prints errors and exits 1.
- `bin/mappamente_desktop` — GUI-subsystem binary on Windows release builds;
  accepts an optional map path; shows a native error dialog on startup failure.
  The Desktop shortcut targets this binary.

The GUI and SVG renderer share the core layout and `Palette`, so exported maps
match the canvas.

## GUI state and interaction

`MappamenteApp` owns:

- `Editor` (document + undo history), current path;
- view state: `kind`, `theme`, palette, zoom, view center, fit flag;
- derived state: cached `Layout`, custom node colors;
- interaction state: selected node, inline `EditingState`, `DragState`;
- native window title and close guard.

Document mutations always go through `Editor::apply` or `apply_coalesced`.
`after_change` recomputes layout/colors, syncs `kind`/`theme` from the document,
and prunes ids that disappeared after undo/delete.

- **Inline edit**: double-click or `F2` opens a foreground `Area` over the node
  with a single-line `TextEdit`; Enter or focus loss commits, Escape cancels.
  Newly added nodes start editing immediately; an empty new node is discarded.
- **Add**: `Tab` creates a child, `Enter` inserts a sibling at `selected + 1`.
- **Delete**: removes the branch, selects its parent; root is protected.
- **Drag**: pointer down on a node starts a coalesced `set_pos` drag; on release
  the drop target is found **under the dragged branch** (the moving node covers
  it), then `move_node` + `set_pos(None)` reparents. Dragging the background
  pans.
- **Save**: commits pending text, enforces `.mappa` for Save As; importing a
  `.md` file forces a native Save As rather than silently writing a lossy
  outline.
- **Layout/theme** are document properties: switching them applies an edit,
  updates the map, persists on save and participates in undo.
- **Close**: dirty documents prompt before closing; the close command is
  cancelled on "No".

## Data model

One tree per document; child order is display order. Node ids are stable short
strings (`n_` + 8 base-32 chars) because the future MCP/AI API addresses nodes
by id and prompt tokens matter.

`pos` is an absolute top-left coordinate. During layout, a manual position
translates the node and its whole subtree, so manual placement composes with
automatic layout.

## Layout pipeline

1. Mirror the document into lightweight nodes (wrapped lines + size); collapsed
   branches are not mirrored.
2. Compute subtree heights bottom-up.
3. Place nodes:
   - Tree: root centered, children alternate left/right, each side is a tidy
     column layout; left output is mirrored. Direct children are depth 1.
   - Radial: spans by leaf count, rings sized so circumcircles never overlap.
   - Logical: tidy columns to the right.
4. Apply manual subtree translations.
5. Compute bounds and an id → index map.

Coordinates are y-down, units are logical pixels, layout is deterministic.

## Undo/redo

Whole-document snapshots per transaction. `Editor::apply` snapshots, runs the
closure, rolls back exactly on error, bumps a monotonic revision and clears
redo. `apply_coalesced` merges repeated same-label edits inside a time window
(for drags). Dirty state compares current revision with the last saved one.
Discrete edits never coalesce with drags.

## Persistence

`json::save` validates, serializes pretty JSON with a trailing newline, writes
a temp file in the destination directory, `sync_all`s and renames over the
target (retries on Windows). Loading validates schema version and invariants.
Unknown fields are ignored for forward compatibility. Markdown and SVG are
written through the same atomic helper.

## CLI conventions

- Runtime errors → stderr + exit 1; clap argument errors → exit 2.
- Missing extensions are filled in: `.mappa` for `new`/`convert`, `.svg` for
  `render`.
- Missing parent directories are created.
- Existing outputs require `--force`.
- `convert` supports Markdown, JSON and SVG; `render` is the dedicated SVG
  path with theme/layout/margin options.

## Planned AI/MCP data flow

```
external MCP client ──stdio──> mappamente mcp ──atomic writes──> .mappa
                                                        │
                                                  file watcher
                                                        ▼
                                                      GUI
in-app AI panel ──tool calls──> shared tool registry ──> live document
                                     ▲
                               same schemas
```

The tool registry (planned `mappamente-core::tools` or a `mappamente-tools`
crate) defines schemas once; `rmcp` and the AI function-calling loop consume the
same definitions. The AI panel calls the handlers in-process, avoiding a socket.
