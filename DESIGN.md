# DESIGN.md

Everything visual: UI, GUI, and UX. What the product looks like, and which visual choices are mandatory rather than left to the agent's taste. For textual tone (not layout/visuals), see [COPY.md](COPY.md).

**Source of truth: Roon's own official app UI**, sampled from user-supplied screenshots (light and dark) — not a Figma file or independent mockup. Values in this file are measured from those screenshots, not eyeballed; a mismatch is a bug report, not a style debate.

This product also has surfaces Roon's official app doesn't have at all: zoomable tile/grid view and list+detail view. These have no screenshot to sample directly — they must still be built from the same tokens/patterns sampled from Roon's real screens below, so they read as if Roon shipped them itself, not as a bolted-on skin.

## Visual language

- Both light and dark themes are first-class from day one, not a "dark mode later" afterthought. Unlike Roon's own app (which requires a restart to apply a theme change), theme switching in this app must happen live, at runtime — no restart.
- New surfaces without a Roon equivalent (zoom/tile view, list+detail view) reuse the same tokens, spacing, and type rules sampled from Roon's real screens — never a separate "our own feature" visual style.
- GPU-accelerated, virtualized grid/list rendering (via Slint, see TECH_STACK.md) for smooth scrolling through thousands of tiles — this is a hard requirement, not a nice-to-have, given the target library sizes.
- Art is requested at the resolution the moment needs (small thumbnails in a dense grid, large images when zoomed in), re-fetching the same `image_key` at a larger `width`/`height` rather than upscaling a cached thumbnail.

## Color tokens

TBD — pending user-supplied screenshots of Roon's official light and dark UI to sample from. Once supplied, fill in both theme tables below in the same format:

```
:root[data-theme="light"] { --bg: ...; --surface: ...; --accent: ...; --text-primary: ...; }
:root[data-theme="dark"]  { --bg: ...; --surface: ...; --accent: ...; --text-primary: ...; }
```

| Pair | Theme | Ratio | Verdict |
|---|---|---|---|
| *(TBD once tokens are sampled)* | | | |

Check every text/background pairing against WCAG 2.1 AA (4.5:1 normal text, 3:1 large text/non-text) in **both** themes before adopting a token. Flag any token that only clears the 3:1 non-text threshold as decorative/border/icon-only — never a text color.

## Typography scale

TBD — pending screenshots. Fill in once sampled:

| Role | Size | Weight | Case/spacing |
|---|---|---|---|
| *(TBD)* | | | |

## Layout

Two primary content-view modes, per decision in the research doc:
- **Grid/tile view** — zoomable; tile size and art resolution scale together (see Visual language above).
- **List view** — adds extra info fields per row not shown in the grid.

Sidebar categories mirror Roon's own sidebar structure (genres, Tidal, live radio, listen later, tags, history, albums, artists, tracks, composers, compositions, my live radio, folders, playlists) — concrete layout TBD pending screenshots.

A separate, app-owned settings screen (not Roon Core's settings) covers: Roon Core connection, Last.fm, view settings, and the light/dark mode toggle. Concrete layout TBD pending screenshots.

## Components

- **Love/unlove toggle** — binary, surfaces Roon's own native track-love control exactly as Roon presents it (no star/half-star granularity). Roon already syncs this to Last.fm on its own, so no app-side rating storage or matching logic is needed. Since it has a direct Roon equivalent, sample its visual treatment from screenshots like any other Roon-parity control (see Visual language) — unlike the genuinely new zoom/list surfaces, this isn't a net-new style to invent.
- **Grid tile** — art + zoom-responsive sizing (see Layout).
- **List row** — art thumbnail + extra info fields (see Layout).
- **Theme toggle** — must apply instantly, no restart (see Visual language).

Full default/hover/focus/disabled states per component: TBD pending screenshots.

## Icons

Hand-rolled inline SVG — the icon set is small and fixed (playback controls, love/unlove, zoom in/out, grid/list toggle, settings, sidebar categories). No icon-library dependency. Revisit only if a later phase needs a materially broader icon set.

Concrete inventory: TBD, finalized once the component list above is locked down.

## Integration with the Roon image API

Art rendering depends on `node-roon-api-image`'s `scale`/`width`/`height`/`format` parameters (see TECH_STACK.md and the research doc) — any component displaying art must request the resolution appropriate to its current on-screen size rather than always requesting one fixed size, since re-requesting a larger size for the same `image_key` is the supported way to get a sharper image on zoom.

## Accessibility

- Every interactive element keyboard-reachable, in an order matching visual order.
- Focus visible via more than a color change alone (pair with an outline/glow) — in both themes.
- Color is never the sole signal for state (e.g. love/unlove) — pair with shape or icon change.
- Contrast checked in both light and dark themes independently — passing one theme doesn't imply the other passes.

## Checklist for new UI

- [ ] New markup/styling uses the tokens and component patterns above, not ad hoc values.
- [ ] Contrast checked for any new text/background pairing, in both light and dark themes.
- [ ] Keyboard/focus behavior checked for any new interactive element.
- [ ] Any new surface without a direct Roon equivalent still reads as visually consistent with the sampled Roon tokens, not a distinct "our own" style.
