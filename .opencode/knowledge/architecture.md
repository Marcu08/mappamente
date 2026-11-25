# Architecture

## Workspace

### `mappamente-core` (implemented, no GUI dependencies)

- `model` — `MindMap`, `Node`, `Link`, `LayoutKind`, short id generation, tree
  operations, validation, outline rendering. `MindMap::edit` applies a closure
  to a clone, validates and touches it, then commits; failures leave the
  document untouched. `outline_with_ids` adds stable ids and compact metadata
  for tools.
- `layout` — deterministic placement, text wrapping, metrics, manual offsets.
  `Tree` is bilateral, `Radial` spreads by leaf weight, `Logical` grows right.
  Layout depth equals document depth for all algorithms.
- `history` — `Editor`, snapshot undo/redo, transactions, drag coalescing,
  dirty tracking by monotonic revision.
- `io::json` — pretty stable JSON, `migrate` hook, validation, atomic save/load,
  5 MiB input limit (`MAX_MAP_FILE_BYTES`).
- `io::markdown` — title + indented bullet outline; exactly one root; lossy.
  `insert_markdown_children` parses an outline, discards the parsed root when it
  has children, and inserts the remaining subtrees with fresh ids.
  `io::load_path`/`save_path` dispatch by extension.
- `export::svg` — standalone SVG, same layout/palettes as the GUI, escaped
  labels, shadows, wrapped text.
- `theme` — four palettes (`dark`, `light`, `paper`, `ocean`) as hex strings,
  hex parsing and contrast helpers.
- `geom` — `Point`, `Size`, `Rect`, edge anchors.

### `mappamente` (lib + two binaries)

- `cli.rs` - clap definitions and command implementations. No command opens
  the editor; otherwise `new`/`convert`/`render`/`gui`.
- `style.rs` - semantic tokens (surfaces, text, accent, grid, shadows, branch
  hues), radii, spacing, type scale and `egui` style application.
- `icons.rs` - painted vector icons for the floating toolbar; no icon font.
  Each icon is authored on a 24×24 logical grid so the glyphs stay symmetric,
  straight and crisp when scaled.  Undo/redo use curved arrows, layout a
  branching tree, theme a half-filled circle and grid a 3×3 dot matrix.
- `ui_prefs.rs` - user-local theme/grid/motion preferences under the platform
  config directory. It never writes next to maps.
- `gui.rs` - native editing canvas:
  - floating pill toolbar: vector icons, shortcut tooltips, accent state for
    the AI panel; shifts over the canvas and hides secondary actions when the
    AI panel is open;
  - central canvas: adaptive dot grid, branch-coloured cubic Bézier edges,
    pill nodes with branch depth tints, hover/selection halos, inline editor;
  - bottom status pill: file state, zoom and node count; errors surface there;
  - pan/zoom/fit, drag move, drag-to-reparent, keyboard editing, save/discard
    guards.
- `bin/mappamente` - console binary (`src/main.rs`), prints errors and exits 1.
- `bin/mappamente_desktop` - GUI-subsystem binary on Windows release builds;
  accepts an optional map path; shows a native error dialog on startup failure.
  The Desktop shortcut targets this binary; `scripts/install-shortcut.ps1
  -Portable` copies it to the requested folder and `scripts/embed-icon.ps1`
  embeds `assets/mappamente.ico` into the copy.

The GUI and SVG renderer share the core layout and `Palette`; `style.rs` maps
the four palettes to branch tints, and the SVG exporter reuses the same branch
palette and readable text contrast.

### `mappamente-mcp` (stdio MCP server)

- `store` — `MapStore` owns one canonical directory. Map names must be simple
  `.mappa`/`.json` file names; existing paths are canonicalized and checked
  against the root, so traversal and escaping symlinks are rejected. It exposes
  list/load/create/add/update/move/delete/layout/export operations.
- `server` — rmcp `#[tool_router]`/`#[tool_handler]` server with ten tools,
  typed `schemars` inputs and structured JSON results. Domain errors become
  `CallToolResult::error` with `{code, message}`; they never panic or turn into
  protocol errors.
- `main` — clap `--dir`, stderr diagnostics, tokio multi-thread runtime and the
  rmcp stdio transport. stdout carries only JSON-RPC.
- Mutations load, compare optional `expected_modified_at`, run a transactional
  `MindMap::edit`, then save atomically. Text/note (10,000 chars), Markdown
  (100,000 chars) and file size (5 MiB) limits are enforced before writing.

### `mappamente-ai` (provider layer, no GUI)

- `provider` — preset table (OpenAI, Anthropic, OpenCode Console/local,
  OpenRouter, Groq, DeepSeek, Mistral, Gemini, xAI, Ollama, LM Studio, custom)
  with protocol, default endpoint/model, key environment variable and key
  requirement.
- `client` — blocking `reqwest` clients for OpenAI-compatible
  `/chat/completions`, Anthropic `/v1/messages` and OpenCode local
  `/api/experimental/generate/text`. Validates URLs, enforces HTTPS for remote
  hosts, applies timeouts and limited redirects, parses text/usage and truncates
  provider errors. No key is logged.
- `credential` — OS keyring (`keyring` 4.2) first, per-provider environment
  variable fallback; save/clear operations are explicit.
- `action` — `Plan`/`MapAction` JSON protocol: add node/tree/subtree, update,
  move, delete, layout, title, theme. `apply_actions` resolves temporary ids,
  enforces 10k/100k limits and returns created/changed ids. Callers run it inside
  one `Editor::apply` transaction.
- `prompt` — strict system prompt and map outline (120,000-character cap).

## GUI state and interaction

`MappamenteApp` owns:

- `Editor` (document + undo history), current path;
- view state: `kind`, `theme`, semantic `Tokens`, zoom, view center, fit flag;
- derived state: cached `Layout`, parsed custom node colors, branch membership;
- interaction state: selected/hovered node, inline `EditingState`, `DragState`,
  per-node appearance timing;
- AI state: `AiPanel` (provider/model/base URL, key status, chat entries,
  `mpsc` receiver), panel visibility and last width;
- user preferences (`UiPrefs`: theme, grid, animations) and system theme;
- native window title and close guard.

Document mutations always go through `Editor::apply` or `apply_coalesced`.
`after_change` recomputes layout/colors/branches, syncs `kind`/`theme` from the
document, and prunes ids that disappeared after undo/delete. `style::apply` is
only re-run when the theme or motion preference changes; colors are parsed once
in `after_change`, never per frame.

The AI panel is restyled around the same tokens: rounded bubbles, a composer
anchored to the panel bottom and provider settings collapsed unless no key is
available. Opening/closing it keeps the current zoom and centre; the fit
command remains available when the reduced canvas needs a fresh frame.

AI flow: `AiPanel` snapshots the map, builds the prompt, starts a plain worker
thread with `reqwest::blocking`, and polls an `mpsc` channel from the UI. A
parsed `Plan` is returned to the app, applied in one `Editor::apply("AI edit")`
call, then `after_change` repaints the canvas. A failure is reported in the
panel and leaves the document untouched.

Keyboard shortcuts are skipped while a text widget has focus, so typing in the
AI prompt, model or base-URL fields does not trigger canvas edits such as
`Tab`/`Enter`/`Delete`.

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
target (retries on Windows). Loading goes through `migrate`, validates schema
version and invariants, and rejects files larger than 5 MiB. Unknown fields are
ignored for forward compatibility. Markdown and SVG are written through the
same atomic helper. AI provider keys live in the OS keyring or environment, not
in `.mappa` files.

## CLI conventions

- Runtime errors → stderr + exit 1; clap argument errors → exit 2.
- Missing extensions are filled in: `.mappa` for `new`/`convert`, `.svg` for
  `render`.
- Missing parent directories are created.
- Existing outputs require `--force`.
- `convert` supports Markdown, JSON and SVG; `render` is the dedicated SVG
  path with theme/layout/margin options.

## MCP and AI data flow

```
external MCP client ──stdio──> mappamente-mcp ──atomic writes──> .mappa
                                      │
                                sandboxed --dir
                                      │
                              (planned) file watcher
                                      ▼
                                    GUI
in-app AI panel ──JSON plan──> mappamente-ai ──HTTPS──> provider
                                      │
                                Editor::apply (one undo step)
                                      ▼
                                live document
```

The MCP crate owns the file-based tools. The AI panel uses a separate in-memory
action protocol for now; unifying both behind one registry remains the target
from D3. External agents edit maps through atomic file writes; live reload is
not implemented yet.
