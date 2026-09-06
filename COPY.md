# COPY.md

How the product sounds, textually: tone of voice, terminology to use or avoid. This governs *wording*, not layout (see [DESIGN.md](DESIGN.md) for visual rules).

## Tone

For any string that has a direct Roon equivalent (menu labels, browse categories, playback controls, settings labels), **copy Roon's own text verbatim** — no independent tone-of-voice pass on that text, the same way DESIGN.md samples Roon's visuals rather than reinterpreting them.

For genuinely new surfaces with no Roon equivalent (the star rating, zoom/list view, this app's own settings screen, connection/error messages this app generates itself), no separate tone has been specified beyond the voice rule below — default to neutral and factual until stated otherwise.

## Terminology

Reuse Roon's own vocabulary exactly for any concept that maps 1:1 to something Roon already has a name for — e.g. **Core**, **Zone**, **Extension**, **Tag** — rather than coining alternate terms. Only invent new terminology for concepts genuinely unique to this app (the star rating, the love/unlove toggle as distinct from the star rating, the zoom/list view modes).

## Voice rules

- Describe the problem, don't blame the user: e.g. "Roon Core not found on the network" rather than "You need to connect to a Core." No second-person imperative in error messages.
- This rule applies to copy this app authors itself (errors, empty states, its own settings screen) — it doesn't apply to text copied verbatim from Roon per the Tone section above.

## Examples

| Situation | Avoid | Use |
|---|---|---|
| Core unreachable | "You need to connect to a Core." | "Roon Core not found on the network." |
| Rating a track with no Roon equivalent | *(no Roon string to copy — author fresh, neutral, no exclamation marks)* | |

Further examples: TBD as concrete UI strings get written — add each new authored string (not copied from Roon) here once it exists, so this table stays a real reference rather than a hypothetical one.
