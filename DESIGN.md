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

Sampled 2026-09-08 from 48 user-supplied screenshots of the real Roon desktop app (26 dark, 22 light,
`~/Pictures/Screenshots`, timestamps `Screenshot_20260908_160535` through `_161303` = dark, `_161618`
onward = light) plus two named dialog captures. Values are direct pixel/histogram reads (ImageMagick,
`convert ... histogram:info:`) of specific UI elements (sidebar nav text, list-row text, dropdown fills),
not eyeballed — per this file's sourcing rule. Anti-aliasing means each read has a small spread (noted
inline); the value given is the cluster peak.

```
:root[data-theme="light"] {
  --bg: #FFFFFF;             /* main content canvas */
  --surface: #FAFAFA;        /* sidebar / elevated panel */
  --surface-alt: #F6F6F6;    /* empty-state card fill, sits on --bg */
  --accent: #7D7DF3;         /* active nav, loved-heart fill, link text, primary buttons (observed range #7D7DF3-#8888F4) */
  --accent-selected-bg: #D9D9F5; /* pale-lavender fill for a selected row in a dropdown/menu */
  --text-primary: #3A3A3D;   /* NOT pure black */
  --text-secondary: #8F8F8F; /* metadata, column headers, nav section labels (observed range #8F8F8F-#ABABAB) */
  --border: rgba(0,0,0,0.08);/* hairline row/column dividers — close to --bg, not a strong line */
}
:root[data-theme="dark"] {
  --bg: #161616;             /* main content canvas */
  --surface: #222222;        /* sidebar / elevated panel — lighter than --bg, not darker */
  --accent: #686CD5;         /* active nav, loved-heart fill, link text, primary buttons */
  --accent-selected-bg: #3A3C68; /* muted-indigo fill for a selected row in a dropdown/menu */
  --text-primary: #FFFFFF;   /* pure white */
  --text-secondary: #919191; /* metadata, column headers, nav section labels */
  --border: rgba(255,255,255,0.06); /* hairline row/column dividers */
}
```

Both themes share one accent hue (blue-violet, R≈G, B substantially higher) sampled from three
independent sources that agree with each other: the active sidebar item, the "loved" heart icon on a
track row, and primary/CTA buttons ("Play now") — treat these as one token, not three coincidentally
similar colors. `--surface` is consistently the *elevated* shade relative to `--bg` in both themes (a hair
lighter in dark, a hair darker in light) — sidebars/panels read as "raised," never as a darker recess.

| Pair | Theme | Ratio | Verdict |
|---|---|---|---|
| `--bg` / `--text-primary` | dark | 18.10:1 | AA normal ✅ |
| `--bg` / `--text-secondary` | dark | 5.74:1 | AA normal ✅ |
| `--bg` / `--accent` (as link text) | dark | 4.05:1 | **Fails AA normal-text (4.5:1); clears large-text/non-text (3:1) only** |
| `--surface` / `--text-primary` | dark | 15.91:1 | AA normal ✅ |
| `--surface` / `--text-secondary` | dark | 5.05:1 | AA normal ✅ |
| `--bg` / `--text-primary` | light | 11.34:1 | AA normal ✅ |
| `--bg` / `--text-secondary` | light | 3.23:1 | **Fails AA normal-text; clears large-text/non-text only** |
| `--bg` / `--accent` (as link text) | light | 3.44:1 | **Fails AA normal-text; clears large-text/non-text only** |
| `--surface` / `--text-primary` | light | 10.86:1 | AA normal ✅ |

**Flagged, not resolved**: Roon's own UI uses `--accent` for small link-style text (artist/album names in
list rows) and `--text-secondary` for metadata in *both* themes at sizes that don't clear WCAG AA's 4.5:1
normal-text threshold — this file's own checklist says a token that only clears 3:1 should be
decorative/icon-only, never a text color, but that's exactly how the sampled source app uses these two.
This is a real tension between the pixel-parity goal (CLAUDE.md era priority, explicit user instruction)
and this file's own accessibility bar. Not resolved here — flagged for a decision when Phase 2+ of
IMP_UI_SHELL.md builds the components that need it: match Roon exactly (accept the gap) or nudge these
two tokens' lightness slightly darker/lighter until they clear 4.5:1 while keeping the same hue.

## Typography scale

Sampled 2026-09-08, same screenshot set. **Two distinct type families, used consistently across every
sampled screen** — not a stylistic accident on one page:

- **A serif display face** for every page title ("My Tracks", "My Albums", "Folders", "Genres", ...) and
  for section headings inside overlays (the "Lyrics" heading in the now-playing view). Regular weight,
  not bold — a fairly thin stroke contrast, elegant/editorial in character.
- **A sans-serif face** for everything else: sidebar nav, list/table content, buttons, metadata, lyrics
  body text.

**Flagged, unconfirmed**: the exact typeface names are unknown — Roon likely licenses both rather than
using a common open font, and no metadata/font-matching tool was run against the screenshots. Recommend
picking a comparable open-source pairing for implementation (e.g. a humanist/editorial serif such as Lora
or Source Serif 4 for headings, a geometric/humanist sans such as Inter for everything else) and treating
it as a deliberate substitution, not an attempt at an exact font match.

Pixel measurements below are as captured in the screenshots (unknown source display scale factor — the
window's own logical pixel size is not recoverable from a raster capture) — use the **ratios**, not the
absolute px counts, when translating into Slint's logical units:

| Role | Measured | Weight | Case/spacing |
|---|---|---|---|
| Page title (serif) | ~56px glyph box height | Regular | Sentence case |
| Section label (BROWSE / MY LIBRARY / PLAYLISTS / FAVORITES / FOLDERS) | small, well under body size | Semi-bold | ALL CAPS, letter-spaced |
| Body / list content (nav items, track titles, button labels) | baseline unit — list-row height measured at ~80px including a ~56px square art thumbnail plus padding | Regular; track titles read slightly heavier than nav items | Sentence case |
| Caption / metadata (page subtitle counts, column headers, durations, artist byline under a now-playing track title) | smaller than body | Regular | Sentence case |

Approximate ratio: page title ≈ 3.5-5× the body/caption size (the exact multiple is confounded by
anti-aliasing at the edges of both measurements — treat "clearly a large display face, clearly bigger
than everything else on the page" as the confirmed part, the precise multiplier as a judgment call for
implementation).

## Layout

Two primary content-view modes, per decision in the research doc:
- **Grid/tile view** — zoomable; tile size and art resolution scale together (see Visual language above).
  Sampled from "My Albums": square 1:1 art tiles in a fixed-column-count row (4 visible in an ~860px-wide
  content crop, more off-screen), each tile's own cover art usually carries the visible title/artist text
  baked into the artwork — but the tile *component* still reserves a caption area below the art (album
  title, then artist, two lines, sans-serif, title in `--text-primary`/artist in `--text-secondary`),
  confirmed on tiles whose cover art doesn't already show text prominently.
- **List view** — adds extra info fields per row not shown in the grid. Sampled from "My Tracks": a
  sortable column-header row (`#`, Track, Length, Album artist, Album, ... each optionally with its own
  inline search icon; the currently-sorted column is `--accent`-colored with a caret), then rows of
  `~56px` square art thumbnail + track title (`--text-primary`) + a small "multiple versions" glyph +
  duration + a drag-handle (four-way-arrow) icon + a love-heart icon + artist/album as `--accent`-colored
  link text. Rows are separated by a hairline `--border`, no zebra-striping observed.

**Page header pattern** (confirmed identical on every "My X" library page sampled — Tracks, Albums,
Artists, History, Composers, Compositions, Playlists, Folders, Genres): serif page title, a muted
`--text-secondary` item-count subtitle directly under it, then (on collection pages with playable
content) a split primary button — a solid `--accent`-filled pill "▶ Play now" with a darker attached
chevron segment that opens a play-mode dropdown (shuffle/radio/etc.) — then a "› Focus" filter-breadcrumb
control plus a circular outline "favorite items only" heart-filter toggle. Empty-state pages (sampled:
"My Tags", "Folders", "My Live Radio") drop the play button and instead show a `--surface-alt` rounded
card with a centered outline icon and one or two lines of instructional copy (e.g. "Click the favorite
button on a folder to add it to your favorites").

Sidebar categories (confirmed directly from full-height sidebar crops, both themes, identical structure):
three labeled sections, uppercase `--text-secondary` section headers —
- **BROWSE**: Home, Genres, TIDAL, Live Radio, Listen Later, Tags, History
- **MY LIBRARY**: Albums, Artists, Tracks, Composers, Compositions, My Live Radio, Folders
- **PLAYLISTS** (header has its own collapse-chevron, `+` add, and `···` overflow controls): the user's
  own playlists, flat list, no further grouping observed

Each row is an icon + label; the active route is `--accent`-colored (both icon and text), inactive rows
are `--text-secondary`. Above the sections: a "roon" wordmark, a gear (settings) icon, and a `···` overflow
menu. Below the sidebar's scrollable area: a persistent mini now-playing strip (art thumbnail + title +
artist), which is the sidebar's visual anchor to the main transport bar below it.

A separate, app-owned settings screen (not Roon Core's settings) covers: Roon Core connection, Last.fm,
view settings, and the light/dark mode toggle. **Layout sampled 2026-09-08** from 8 screenshots of Roon's
own Settings screen (light + dark) — captured deliberately for chrome/layout only, not content: Roon's
actual settings are Core-internal and permanently out of scope per CLAUDE.md's mandatory technical
choices, so nothing about *what* these screenshots configure carries over, only *how the screen is built*:

- **Two-level navigation**: a bold "Settings" title, then a flat list of section names below it (General,
  Storage, Services, Setup, ..., About) in a dedicated sub-sidebar column between the main sidebar and the
  content pane — same active-item treatment as the main sidebar (accent color) *plus* an accent underline,
  a stronger emphasis than the main sidebar uses (which relies on color alone). Given this file's own
  accessibility rule ("color is never the sole signal for state"), copy the underline too, not just the
  color, when building this sub-nav.
- **Section-grouped rows**: uppercase `--text-secondary` group headers (e.g. "BROWSING PREFERENCES") above
  clusters of rows, each row a label (+ optional smaller `--text-secondary` description line beneath it)
  on the left and a control on the right — a toggle, a "Value ▾" dropdown, a text input, or a button.
- **A connection-status card pattern** (sampled from Roon's own "ROON SERVER" card): icon + name + a
  metadata line (address) + status text, with action buttons (primary/secondary, see Components) at the
  trailing edge and a dismiss/disconnect action at the card's top-right corner. This is the direct model
  for our own settings screen's Core-connection section — swap Roon-account fields for
  `core::roon::connection`'s own `ConnectionState`/paired-Core-id.

No further screenshot needed before Phase 5 — see Components below for the new controls this uncovered
(toggle switch, primary/secondary buttons, text input).

## Components

- **Love/unlove/ban control** — binary love/unlove toggle plus a ban (thumbs-down/skip-and-don't-replay) control, treated as one group per user decision (2026-09-08), surfacing Roon's own native controls exactly as Roon presents them (no star/half-star granularity). Roon already syncs love/unlove to Last.fm on its own, so no app-side rating storage or matching logic is needed. Since these have a direct Roon equivalent, sample their visual treatment from screenshots like any other Roon-parity control (see Visual language) — unlike the genuinely new zoom/list surfaces, this isn't a net-new style to invent. Backend wiring is deferred (see IMPL_UI_SHELL.md); this entry covers visual treatment only.
  - **Confirmed placement/visual**: a heart-outline icon per track row in list views ("My Tracks" sampled
    in both themes) — outline-only (`--text-secondary`) when not loved, filled/stroked in `--accent` when
    loved. A circular outline heart button also appears in every page header as a "favorite items only"
    filter toggle (same glyph, different purpose — don't conflate the two visually).
  - **Love/ban placement resolved** (2026-09-08, `love_unlove_ban_menupopup.png`): the heart also appears
    in the **expanded now-playing overlay's** top icon row (filled `--accent` when loved, same treatment
    as the list-row heart) — not in the persistent bottom mini transport bar, which has no love/unlove/ban
    presence at all (see Bottom transport bar below). Ban has **no standalone icon anywhere**: it's "Ban
    this track", the last entry in that track's "•••" overflow/more-actions menu (opened from the same
    overlay), alongside mostly out-of-scope items (add to tag, view lyrics, view TIDAL info, share, export,
    edit) that reach beyond this app's scope. Practical effect: the bottom bar (IMPL_UI_SHELL.md Phase 3)
    needs no love/unlove/ban treatment at all; the heart icon and the "Ban this track" menu item belong to
    the expanded now-playing overlay, a still-unbuilt surface — see Open work in CURRENT_STATE.md.
- **Grid tile** — square 1:1 art, zoom-responsive sizing (see Visual language), optional two-line
  title/artist caption below when the art itself doesn't already carry that text (see Layout).
- **List row** — `~56px` art thumbnail + title (`--text-primary`) + accent-linked artist/album fields +
  duration + drag-handle + love icon, hairline `--border` between rows, no zebra-striping (see Layout).
- **Page header** — serif title + muted count subtitle + split "Play now ▾" primary button (`--accent`
  fill, darker attached dropdown segment) + Focus/favorite-filter row (see Layout's page header pattern).
- **Empty state** — a `--surface-alt` rounded card, centered outline icon, one to two lines of muted
  instructional copy, no title (see Layout).
- **Dropdown/menu** (zone picker, output menu) — a `--surface`-toned panel; a hovered/selected row gets
  `--accent-selected-bg` fill with primary-colored text/icon, not a border or underline. Sampled from the
  zone-switcher popup ("Xonar" zone + "Pause all") and the per-output settings menu (standby/DSP/group/
  settings icons — see Icons below for which of these are actually in scope). **Confirmed placement**
  (2026-09-08, user correction): Roon has no persistent/standing zone list anywhere in its UI — this
  popup is the *only* place a zone list appears. Any future implementation of a zone list must live
  here, not in the sidebar or as a separate screen. **Trigger confirmed 2026-09-08 (user correction)**:
  opened by clicking the output-icon in the bottom transport bar's bottom-right corner, not the
  zone-name label next to it — the icon is always present at a fixed size, unlike the name (blank
  until a zone is selected). **Row content re-sampled with pixel precision 2026-09-08**
  (`Screenshot_20260908_185248.png`): each zone row is icon + name + an inline pause/play button at the
  row's trailing edge, all within the `--accent-selected-bg` highlight (measured `#434581`, matching the
  already-sampled dark-theme token `#3A3C68` closely) when that zone is selected/active; a separate
  "Pause all" row (icon + label, no selected-state styling — an action, not a selection) sits below the
  zone list.
- **Bottom transport bar** — persistent, full-width, `--surface`-toned. Left: art thumbnail + track title
  (`--text-primary`, bold) + artist byline (`--text-secondary`) below it, or "Nothing playing" in
  `--text-secondary` when idle. Center: a signal-path indicator (see Icons), prev/play-pause/next icon
  buttons + a play-queue icon, then a seek bar (`#606060` unfilled track — re-sampled 2026-09-08,
  lighter than first assumed — `--accent`-filled played portion measured `#6A6ED9` ≈ the sampled
  `--accent` token, circular thumb, flanked by elapsed/remaining time in `--text-secondary`) — controls
  render idle/dim when nothing is playing. Right: the output-icon (**clicking it opens the
  zone-switcher popup** described above under Dropdown/menu — trigger corrected 2026-09-08, see that
  entry) with the selected zone's name centered *below* it (re-sampled 2026-09-08 — not beside it as
  first assumed), then a volume icon opening the volume popover.
- **Toggle switch** (sampled 2026-09-08 from Roon's own Settings screen — layout/chrome only, see
  Layout's Settings note) — a pill track with a circular thumb. On: `--accent` fill (`#686CD5` dark /
  `#8787F5` light, matching the accent already sampled elsewhere), white thumb, positioned right. Off: a
  neutral mid-grey fill (`#3B3B3B` dark, `#DBDBDB` light — a dedicated "control-off" shade, distinct from
  both `--surface` and `--bg`), a slightly darker/lighter thumb of the same hue (low-contrast against its
  own track by design — Roon doesn't rely on the thumb alone here, the adjacent "Yes"/"No" text label
  carries the state), positioned left. Always paired with a text label ("Yes"/"No" in the samples) per
  this file's own "color is never the sole signal for state" rule. This is the direct model for
  DESIGN.md's light/dark theme toggle.
- **Buttons — primary vs. secondary** (same source): primary actions ("Play now", "View account info",
  "Enable") are solid `--accent`-filled pills with white/primary-on-accent text. Secondary/lesser actions
  ("Find", "Clear cache", "Add HQPlayer", "Logout") are a muted cool-grey pill (`#C5C9D1`-ish light theme,
  tinted slightly toward the accent hue rather than neutral grey) with `--text-primary` text — visually
  quieter but still a filled pill, never a plain text link, for anything that's a real action.
- **Text input** — a white/`--bg`-toned field with a thin `--border`-ish grey outline and a large,
  roughly-half-height corner radius (not a full pill). No inset/focus style was caught in the sampled
  screenshots — flagged as unconfirmed, same as other hover/focus states in this file.
- **Settings row** — see Layout's Settings section: label (+ optional muted description line) left,
  control right, section-grouped under an uppercase `--text-secondary` header.
- **Theme toggle** — must apply instantly, no restart (see Visual language). Built from the Toggle switch
  component above.

Full hover/focus/disabled states per component: mostly still TBD — static screenshots only reliably
caught one dropdown's hover state (the `--accent-selected-bg` row above) and the idle/disabled look of
transport controls with nothing playing. Deliberate hover/focus/disabled screenshots would need a future
follow-up if closer parity on those states matters before Phase 2+ of IMPL_UI_SHELL.md builds them.

## Icons

Hand-rolled inline SVG — the icon set is small and fixed (playback controls, love/unlove, zoom in/out, grid/list toggle, settings, sidebar categories). No icon-library dependency. Revisit only if a later phase needs a materially broader icon set.

**Concrete inventory, confirmed from screenshots (2026-09-08)** — all simple outline-style glyphs, no
filled/duotone icons observed except the loved-heart's filled accent state:

Sidebar: home, genre/tag-stack, TIDAL's own diamond-cluster mark, radio-waves (reused identically for
both "Live Radio" and "My Live Radio"), clock-with-tag (Listen Later), price-tag (Tags), list-lines
(History), globe (Albums), two-people (Artists), musical-note (Tracks), piano-keys-with-person
(Composers), lock-like glyph (Compositions), folder (Folders), chevron (section collapse / breadcrumbs),
`+` (add playlist), `···` (overflow menu), gear (settings).

List rows / page headers: search/magnifying-glass (per-column filter), caret (active sort direction),
four-way-arrow (drag-to-reorder), heart outline/filled (love — see Components), circular heart-outline
button (favorite-filter toggle, visually identical glyph to the love icon but a different function —
don't conflate).

Transport bar — **re-sampled with pixel precision 2026-09-08** from `Screenshot_20260908_185235.png`
(bar) and `Screenshot_20260908_185248.png` (zone-switcher popup), both `~/Pictures/Screenshots/`,
superseding the earlier, lower-confidence description:
- A **signal-path indicator** (a glowing white/blue four-point sparkle, left of "previous") — clicking
  it opens a read-only signal-path panel (source format → DSP chain → output device, e.g. "TIDAL FLAC
  44.1kHz" → "MQA Studio" → ... → "Xonar U7 MKII USB Audio"), confirmed via
  `Screenshot_20260908_185311.png`. **Not "Roon Radio"/`auto_radio`** — an earlier note in this file
  guessed that; corrected by the user directly. The panel's *contents* (bit depth conversion, volume
  leveling, parametric/procedural EQ stages) are Core-internal DSP detail, permanently out of scope
  per CLAUDE.md's mandatory technical choices — but the signal-path *display itself* (what format is
  playing, through what chain, to what device) is read-only information, not a DSP control, so it may
  be worth a future protocol study to see whether the official API exposes it at all: no source studied
  so far (`sood-moo.md`/`transport.md`/`browse.md`) covers it, and none of `transport:2`'s modeled types
  (`Zone`/`Output`/`NowPlaying`/`Volume`/`SourceControl`) carry format/bit-depth/sample-rate fields.
  Logged as an open research question in CURRENT_STATE.md, not committed work.
- previous / play / pause / next — simple filled glyphs (triangle+bar, triangle, two bars, mirrored
  triangle+bar), confirming the shapes already hand-drawn for these are conceptually right. Color
  re-sampled with pixel precision 2026-09-10: a consistent `#CCCCCC` across every transport-bar/
  zone-popup/volume-popover icon glyph (prev/play-pause/next/queue/output/volume alike), not the
  earlier vague "light-grey/white" guess split across `#919191`/`#ffffff`.
- A **queue icon**: a small play-triangle followed by three horizontal lines — **corrected 2026-09-10**
  (re-measured pixel-by-pixel against `Screenshot_20260908_185235.png` after a user-reported mismatch):
  the top and bottom lines are the *same* length, and the **middle** line is the odd one out — shorter
  and inset on both sides, not part of a monotonic size progression as the 2026-09-08 pass first read
  it. The triangle itself is small enough (roughly as tall as the three lines combined) to genuinely
  overlap their vertical span, which is why the icon reads as "triangle partly inside the lines" rather
  than two cleanly-separated clusters — a "play queue" glyph, not a generic list/hamburger.
- **Seek bar**: fill `#6A6ED9` (matches the already-sampled `--accent` `#686CD5` closely — no change
  needed), but the **unfilled track is `#606060`**, a visibly lighter mid-grey — not the near-black
  `#333333` first used, which read as barely visible against `--surface`. The thumb riding on it is a
  distinct **white** circle, not accent-colored like the fill (confirmed 2026-09-10, same source).
- **Output icon**: a proper bookshelf-speaker glyph — a rounded-rectangle outline with four small
  corner dots (screws) and two concentric circles inside (tweeter above, woofer below) — not a plain
  filled square. Shape re-checked and confirmed correct 2026-09-10; color is the same `#CCCCCC` as
  every other transport-bar icon (see above).
- **Volume icon**: a speaker-with-sound-waves glyph — shape still not sampled at high enough resolution
  to draw with confidence (a placeholder approximation), but its color is confirmed `#CCCCCC` (2026-09-10,
  same source), consistent with every other transport-bar icon.
- The **zone-switcher popup** (Dropdown/menu, see Components) has more structure than previously
  sampled: each zone row shows the output-speaker icon + zone name + an inline pause/play button at the
  row's trailing edge (all inside the row's `--accent-selected-bg` highlight when selected), and a
  separate **"Pause all"** row below the zone list (icon + label, no selected-state styling — it's an
  action, not a selection).

Output/zone menus: moon (standby — **in scope**, `transport::control::standby` already exists),
waveform (DSP), shuffle-style double-arrow (zone grouping/transfer), gear (device settings) — the latter
three are **out of scope**: they reach into Core-internal Audio Setup/DSP/Zone config, which CLAUDE.md's
mandatory technical choices permanently exclude. Render them only if a future screen needs to visually
match a menu that contains one, never wire them to anything.

**Not yet confirmed**: the now-playing overlay's top icon row includes an unidentified glyph next to the
heart (resembles a radio/broadcast icon) plus a "•••" more-actions menu and a grid-style icon, none
resolved to specific functions at the sampled resolution — needs a closer screenshot before those icons
can be drawn with confidence. Not blocking: none of them are in this app's scope today. Love/ban placement
itself is now resolved, see Components.

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
