# Design direction

## What felt dated
- The top bar was a dense row of text buttons that competed with the map and
  wrapped badly on narrow windows.
- Surfaces used saturated blue-grey panels and heavy 10 px rectangles; the
  status bar repeated information already visible elsewhere.
- Every node used one of four depth fills; sibling branches had no identity and
  hex colors were parsed again for every node on every frame.
- The dot grid was fixed in world space, so fit zoom could produce tens of
  thousands of dots; hover and selection had no feedback.
- The AI panel opened with its provider form expanded, and theme choice reset on
  launch instead of following the OS.

## Direction
A quiet workspace: warm/neutral surfaces, one accent per palette, thin 1 px
borders, soft shadows and generous rounded shapes. The map is the focus; chrome
floats above it instead of taking a permanent band. Interactions use short eased
transitions (about 180 ms) and are disabled by a user setting.

## Tokens
Semantic names live in `crates/mappamente/src/style.rs`; components never use
raw colors. Light values:

| Token | Value | Use |
| --- | --- | --- |
| `bg` | `#F6F6F7` | canvas / panel background |
| `surface` | `#FFFFFF` | floating toolbar, message bubbles |
| `surface_muted` | `#F0F0F3` | inputs, secondary chips |
| `border` | `#E4E4E9` | 1 px separators |
| `text` | `#18181B` | primary text |
| `text_muted` | `#71717A` | hints and labels |
| `accent` | `#2F6FED` | brand accent, root, focus |
| `accent_soft` | `#E6EDFF` | hover/focus halo |
| `grid` | `#D9D9E0` | dot grid |
| `error` | `#D33A3A` | errors |

Dark values use the same names: `bg #0D0D10`, `surface #151519`,
`surface_muted #202027`, `border #2A2A32`, `text #EDEDF0`,
`text_muted #9A9AA6`, `accent #4F8CFF`, `grid #23232B`.
Paper and ocean keep their warm/teal accent but share the neutral structure.

## Type and geometry
- Type scale: 20/15/14/13/11 px for map title, node text, body, buttons and
  hints. egui's default proportional font and its emoji fallback are reused; no
  additional font ships with Mappamente and there is no extra binary-size cost.
- Spacing: 4, 8, 12, 16, 24, 32 px. Panel radius 12 px, control radius 8 px,
  node pill radius = half the node height (capped at 18 px).
- Borders are 1 px; shadows are one soft layer, not per-node stacks.

## Nodes
- Root: accent fill, slightly larger visual rect, accent-contrast text.
- First-level branches: eight curated hues, one per branch, chosen for contrast.
- Descendants: the same branch hue mixed toward the surface as depth grows.
- An explicit `node.color` always wins; text falls back to the best of white or
  near-black for at least AA contrast.
- Hover gets a soft halo; selection gets accent ring + halo; both animate.
- Collapsed nodes keep a small dot/chevron; text uses the shared wrapping.

## Connectors and canvas
- Cubic Bézier curves, branch-colored, width decreasing with depth.
- Dot grid is adaptive: screen spacing is kept between roughly 24 and 72 px,
  so fit zoom cannot explode the dot count. The grid can be toggled off.
- Nodes and edges are culled against the viewport; branch identity is cached,
  and colors are precomputed once instead of parsing hex per frame.
- Pan, zoom and fit stay immediate; fit is eased unless animations are off.

## Chrome
- Floating pill toolbar centered over the canvas: drawn vector icons, tooltips
  with shortcuts and one accent highlight for the active AI panel. It shifts
  left when the AI panel opens and drops secondary actions into the menu.
- Bottom status pill over the map: file state, zoom, node count; errors surface
  there. It never takes a permanent band.
- Theme follows the OS for new and welcome maps; manual choice, grid and motion
  settings persist in the user config directory (never next to maps). Existing
  maps keep their document theme.
- AI panel: same surfaces and radii, rounded message bubbles, focus-ring input,
  composer anchored to the bottom, provider settings collapsed by default
  unless a key is missing.
