# Known issues and limitations

## Current version (0.2.0)

- Node notes, colors and the collapse flag are supported by the format and
  core, but the canvas has no UI to edit them. Existing values are preserved.
- No in-canvas collapse/expand toggle; collapsed branches imported from files
  render as collapsed.
- OPML, FreeMind and PNG/PDF export are not implemented.
- Cross-links (`links`) are reserved and validated but never rendered.
- Single-window only; opening a second file starts a second process.
- External changes trigger a Reload / Keep Local / Save As dialog when local
  edits exist. "Keep Local" deliberately freezes the on-disk base revision so
  later saves cannot silently overwrite the external version.
- Save As uses an atomic compare-and-save with `expected_revision = None`; an
  existing destination produces an explicit overwrite confirmation instead of a
  silent replacement, and a file created by another process between the check
  and the write is detected.

## MCP server

- stdio only: there is no HTTP/SSE transport, so clients must launch the binary.
- Map names are simple file names inside `--dir`; subdirectories are not
  addressable. Windows device names and names with trailing dots/spaces are
  rejected on every platform to keep behavior portable.
- `list_maps` loads every map to read its metadata, so it costs O(total map
  size) per call.
- Link tools are intentionally absent. Existing `links` are preserved on save
  but are not exposed to clients.
- Export supports SVG, Markdown and native JSON only; there is no PNG/PDF.
- The trust model is local: anyone who can launch the server can edit every map
  in `--dir`. Do not point it at a sensitive directory.
- Advisory file locks protect against concurrent writers that use the same
  locking mechanism. A hostile or non-cooperating process can still ignore the
  lock and race the filesystem.

## AI assistant

- **No token streaming:** the reply is parsed and applied after the provider
  response completes. The canvas updates immediately at that point, but text and
  actions do not appear progressively.
- **No native tool calls:** providers are not given function/tool schemas. The
  model must return the documented JSON object; malformed JSON is shown as an
  error and leaves the map unchanged.
- **No retry or cancel:** requests have a 120-second timeout, and the send
  button is disabled until the worker returns. Closing the panel does not cancel
  the request.
- **Context limit:** maps whose outline exceeds 120,000 characters are refused;
  they are never truncated silently. The limit covers the map outline, not the
  full request, so a long chat can still hit a provider context window and
  return an error.
- **UI work:** very large valid action plans are validated and applied on the UI
  thread after the response arrives; this can briefly stall rendering.
- **History is in-memory:** chat turns are not persisted across restarts, and
  only the last 12 user/assistant messages are sent as context. The selected
  provider/model/base URL also reset to preset defaults on launch (keys persist
  in the keyring).
- **Action coverage:** AI actions can edit text, notes, colors, collapsed state,
  structure, layout, title and theme. Manual positions, cross-links, export and
  file open/save are not AI actions.
- **One undo step:** the whole turn is atomic. If any action is invalid, no
  action from that turn is applied.
- **Keys:** the keyring may be unavailable on headless Linux or locked
  desktops; the environment variable remains the fallback. Saving a key can
  prompt the OS credential UI.
- **Privacy:** the current map outline and chat context are sent to the selected
  provider. Remote HTTP is rejected, but the provider's own retention policy
  applies.
- **OpenCode local:** uses the experimental
  `/api/experimental/generate/text` route, the default `opencode serve` port
  4096, and optional Basic auth (`opencode` + `OPENCODE_PASSWORD`). The route or
  port may change between OpenCode versions; both are editable.
- **Provider defaults age:** model ids and endpoints in the presets are starting
  points and may be deprecated; the model and base URL fields are editable.
- **Architecture debt:** MCP file tools and AI actions are separate handlers;
  the shared registry from decision D3 is not implemented yet.
- **Build cost:** `reqwest` with rustls/aws-lc and `keyring` add a noticeable
  first-build time (roughly several minutes on Windows), although runtime
  overhead is small.

## Packaging and shortcut

- The Desktop shortcut targets `target\release\mappamente-desktop.exe`. Running
  `cargo clean` or moving the repository breaks it; re-run
  `scripts/install-shortcut.ps1`. `-Destination <folder> -Portable` copies
  `Mappamente.exe` and its icon shortcut into that folder, so the launcher
  survives a `cargo clean`.
- The default release executable still has no icon embedded in its PE
  resources; the window and taskbar use the runtime icon. The portable copy is
  post-processed by `scripts/embed-icon.ps1` and does get `assets\mappamente.ico`
  embedded, and the `.lnk` also points at that icon.
- Installers, file associations, signing and notarization are future work.
- Pre-built v0.1.0 artifacts are Windows x64 only; other platforms build from
  source.

## Layout and rendering

- Radial layout guarantees non-overlapping boxes by increasing ring radii. Many
  direct children can produce a very large map; there is no compaction yet.
- Text metrics are estimated with `char_width` and `unicode-width`; the GUI and
  SVG may render glyphs slightly wider/narrower than the boxes. SVG uses the
  fixed 15 px font size / 20 px line height from `Metrics`.
- Manual positions are absolute; window resizing does not reflow them.
- Themes beyond the four built-ins fall back to dark in the UI.
- The inline editor is single-line; imported multi-line labels are flattened
  when edited.

## Interaction edge cases

- Drag-to-reparent uses the topmost non-dragged node under the pointer; dropping
  onto a branch that is behind another node may pick the visible one.
- A drag is a coalesced undo transaction; reparenting after the drag adds a
  second transaction, so undo may need two steps to restore the original parent.
- The inline edit overlay is constrained by egui when near the window edges and
  may shift slightly; the text still applies to the correct node.
- Keyboard shortcuts require the canvas/window to have focus (click once after
  launching from automation).

## Editor UI and design

- Theme preference is used for new and welcome maps. Opening an existing map
  keeps the theme stored in that file by design; changing it is a document edit.
- Motion is controlled by a manual **Animations** checkbox; egui 0.37 has no
  OS reduce-motion query. On a very slow machine, animations still cost one
  extra repaint per transition.
- The grid and motion preferences live in the user config directory. There is
  no settings dialog to reset them to defaults or to follow the OS again without
  editing `ui.conf` or choosing another theme.
- On a narrow window with the AI panel open, secondary toolbar actions (Save As,
  Layout, Theme, Grid) move to the overflow menu; keyboard shortcuts still work.
- The status pill is painted over the canvas. With an extreme manual pan it can
  cover a node; the fit command includes enough margin for normal use.
- SVG export now matches the branch palette and pill radius, but it still omits
  the interactive dot grid and uses a static drop shadow.
- Screenshot automation must set DPI awareness before querying window/client
  rectangles; otherwise the bottom of a 125%-scaled window is silently cropped.

## Testing

- 176 tests: 106 core unit, 2 core integration, 15 library unit (CLI/GUI/style/
  preferences/AI panel/file watcher), 7 CLI integration, 14 MCP unit, 2 MCP stdio
  integration, 24 AI unit, 6 AI HTTP tests. Doc tests: 0.
- AI HTTP tests run a local `TcpListener` mock and verify OpenAI-compatible,
  Anthropic and OpenCode-local request paths, auth headers, bodies and response
  parsing without external network access.
- The AI panel unit test starts the real worker thread against a local mock,
  parses the plan and applies it to a live `MindMap`.
- The MCP stdio integration test spawns the real binary, performs the initialize
  handshake, lists the ten tools and exercises create/add/subtree/update/layout/
  export/delete plus sandbox, missing-map and stale-token errors.
- The 1,111-node layout test has a generous 5-second guard in debug builds; it
  is not a micro-benchmark. Release layout measured at ~3 ms on this machine.
- Redesigned canvas, release build, 1,111-node map in fit view: 5.78 ms average
  eframe CPU time per frame, 6.45 ms p95 (240 samples; the 168 ms maximum is the
  first-frame/startup spike). This is CPU-side frame time, not a GPU profile.
- GUI flows were smoke-tested manually with DPI-aware mouse/keyboard automation
  (create, edit, delete, undo, drag-reparent, save). There are no automated UI
  tests yet.
- No property-based testing yet; overlap and determinism tests use fixed
  generated trees.

## Environment

- The local shell starts with an empty `CARGO_TARGET_DIR`, which makes cargo
  fail. Set `CARGO_TARGET_DIR` explicitly when running commands.
- Windows UI automation must set thread DPI awareness before `SetCursorPos`;
  otherwise clicks are scaled and miss node rectangles.
- First release build downloads and compiles eframe/egui/wgpu and takes several
  minutes; incremental builds are much faster.
