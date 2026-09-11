# `com.roonlabs.transport:2` wire-protocol study

Status: study complete, unimplemented. Written to unblock `core::roon::transport` (IMPL_TRANSPORT.md
Phase 0) — before this existed, the fine-grained steps for Phases 2-4 there couldn't be written without
guessing at message shapes, the same reasoning [sood-moo.md](sood-moo.md) documents for
`core::roon::connection`. Nothing in this document is copied verbatim from the sources below; it's an
independent description of the wire format, written from reading and cross-checking them.

**Scope note**: unlike `pairing:1`/`ping:1` (services *this extension provides*, documented in
sood-moo.md), `transport:2` is a service *the Core provides* — this extension is the client. It must be
declared in `required_services`/`optional_services` at registration (`moo::handshake::register`, per
CURRENT_STATE.md's 2026-09-07 fix), and the extension then sends `REQUEST`s against it and receives
`CONTINUE`/`COMPLETE` responses (including long-lived subscriptions) over the same paired MOO connection —
see "Subscription semantics and reconnect" below for why this bears directly on IMPL_TRANSPORT.md's Phase 1
open architectural question.

## Sources studied

| Source | What it is | License | Role here |
|---|---|---|---|
| [`RoonLabs/node-roon-api-transport`](https://github.com/RoonLabs/node-roon-api-transport) | Official JS SDK's transport module, published by Roon Labs | Apache-2.0 | **Primary/normative** — `lib.js` read in full |
| [`RoonLabs/node-roon-api`](https://github.com/RoonLabs/node-roon-api) | Official JS SDK core (the `Moo` request/subscription machinery `lib.js` above calls into) | Apache-2.0 | **Primary/normative** — `moo.js` read in full, for subscription-key and reconnect semantics |
| [`shin1ohno/roon-rs`](https://github.com/shin1ohno/roon-rs) | Community Rust SDK (`roon-api` crate on crates.io) | MIT OR Apache-2.0 | Corroboration + Rust typed-model reference — `crates/roon-api/src/transport.rs` |
| [`TheAppgineer/rust-roon-api`](https://github.com/TheAppgineer/rust-roon-api) | Community Rust SDK (git-only, alpha) | MIT | Secondary corroboration — `src/transport.rs` |

Note: `node-roon-api-transport` is a separate repo from `node-roon-api` (the one sood-moo.md studied) —
Roon Labs ships each service module (`transport`, `image`, ...) as its own npm package layered on the core
SDK. Both are Apache-2.0, same org. All four sources agree on every request/response shape below; two
narrow divergences worth flagging are called out in "Rust design notes".

## Service identity

`SVCNAME = "com.roonlabs.transport:2"`. Every request name below is `SVCNAME + "/" + <method>`, e.g.
`com.roonlabs.transport:2/subscribe_zones`, sent as a MOO `REQUEST` per sood-moo.md's framing. All bodies
are JSON (`Content-Type: application/json`).

## Data model

**`Zone`**:

| Field | Type | Notes |
|---|---|---|
| `zone_id` | string | |
| `display_name` | string | |
| `outputs` | `Output[]` | the outputs currently in this zone (grouping merges multiple outputs into one zone) |
| `state` | `"playing" \| "paused" \| "loading" \| "stopped"` | |
| `seek_position` | number, optional | |
| `is_previous_allowed` / `is_next_allowed` / `is_pause_allowed` / `is_play_allowed` / `is_seek_allowed` | bool | gate whether the corresponding `control`/`seek` call is meaningful right now |
| `queue_items_remaining` / `queue_time_remaining` | number, optional | |
| `settings` | object | `{ loop: "loop"\|"loop_one"\|"disabled", shuffle: bool, auto_radio: bool }` |
| `now_playing` | object, optional | present only when playback is active — see below |

`now_playing`: `{ seek_position?, length?, image_key?, one_line, two_line, three_line }`, where
`one_line = { line1 }`, `two_line = { line1, line2? }`, `three_line = { line1, line2?, line3? }` — three
pre-formatted display strings at increasing detail, for UIs with different amounts of space. No raw
artist/album/track fields are exposed separately; a UI wanting those must use `browse:1`/parse the
`*_line` strings, or an image/metadata service not in scope here.

**`Output`**:

| Field | Type | Notes |
|---|---|---|
| `output_id` | string | |
| `zone_id` | string | back-reference to the zone this output belongs to |
| `display_name` | string | |
| `state` | same enum as `Zone.state`, optional | JSDoc and this study both originally listed this as required, matching `Zone.state` — confirmed absent entirely on a live Core's output whose only `source_controls` entry was `"status": "indeterminate"` (2026-09-11), while the parent zone's own `state` was present in that same body |
| `source_controls` | array, optional | see divergence note below — JSDoc formats this as a singular object but it is really a list |
| `volume` | object, optional | present only for outputs that support volume control |

`source_controls[]` entries: `{ control_key, display_name, status: "selected"\|"deselected"\|"standby"\|"indeterminate", supports_standby: bool }`.
`control_key` is what `standby`/`toggle_standby`/`convenience_switch` accept as an optional selector when
an output exposes more than one source control.

`volume`: `{ type: "number"|"db"|"incremental"|<other>, min?, max?, value?, step?, is_muted? }`. **Values,
bounds, and step are floating point**, not integers, and ranges "can extend below and above zero,
sometimes at the same time" (verbatim from the JSDoc — e.g. a dB range like -80..10). An unrecognized
`type` should be treated like `"number"`. `type: "incremental"` means a +/- only control with no
value/range feedback at all (`min`/`max`/`step`/`value`/`is_muted` absent) — `change_volume` against it
should only ever be called with `how: "relative"` and `value: ±1`.

**`QueueItem`** (from `subscribe_queue`, not part of `Zone`/`Output` themselves):
`{ queue_item_id, length, image_key?, one_line, two_line, three_line }` — same three-line display shape as
`now_playing`.

## Subscription semantics and reconnect

Three subscribable streams: `subscribe_zones`, `subscribe_outputs`, `subscribe_queue`. All three share the
same envelope, implemented once in the reference SDK's `Moo.prototype._subscribe_helper` (`moo.js`), not
reimplemented per-service:

1. The subscriber allocates a `subscription_key` (in the reference implementation: a per-`Moo`-instance
   counter, `self.subkey++` — i.e. scoped to one MOO connection, not global or Core-assigned) and sends it
   in the request body: `REQUEST <svc>/subscribe_zones { subscription_key: N, ... }`.
2. The first response for that `Request-Id` is `CONTINUE Subscribed` with the full current state:
   `{ zones: Zone[] }` for zones, `{ outputs: Output[] }` for outputs, `{ items: QueueItem[] }` for queue.
3. Subsequent `CONTINUE Changed` messages carry only deltas, as whichever of these keys are present (any
   subset, per event — not all keys are sent every time):
   - zones: `zones_added`, `zones_changed`, `zones_removed` (array of `zone_id` strings, not full
     objects), `zones_seek_changed` (array of `{ zone_id, seek_position?, queue_time_remaining }` — a
     separate, higher-frequency channel from `zones_changed` specifically so seek-position ticking doesn't
     require re-sending a whole `Zone`).
   - outputs: `outputs_added`, `outputs_changed`, `outputs_removed` (by `output_id`).
   - queue: `changes: [{ operation: "insert"|"remove", index, items?, count? }]` — an ordered list of
     positional edits, not a full replacement; `items` accompanies `insert`, `count` accompanies `remove`.
4. To end a subscription: `REQUEST <svc>/unsubscribe_zones { subscription_key: N }` (same key), which gets
   its own `COMPLETE`, distinct from the original subscribe request's still-open one — a subscription
   never gets a `COMPLETE` of its own while active, only `CONTINUE`s; the `COMPLETE Unsubscribed` (or
   `Success`, both are used across the sources) closes it out. Nothing in the reference implementation
   sends an unprompted terminal message on the Core's own initiative except via disconnect (next point).

**Reconnect implication (feeds IMPL_TRANSPORT.md's Phase 1 open question directly)**: `moo.js`'s
`Moo.prototype.clean_up` — called when the underlying transport closes — walks the in-flight `requests`
map (keyed by `Request-Id`, populated by `send_request`) and invokes every pending callback with no
arguments, then clears the map. There is no persistence of subscription state across a `Moo` instance: a
new websocket connection means a brand new `Moo` object, a `reqid`/`subkey` counter reset to zero, and an
empty `requests` map. **The Core has no memory of a previous connection's subscriptions either** — nothing
in `lib.js`/`moo.js` re-sends `Subscribed` unprompted on reconnect. Concretely: after any reconnect
(`connection`'s `Discovering → ... → Paired` loop per CLAUDE.md §1), whatever module owns `transport:2`
must **re-issue `subscribe_zones`/`subscribe_outputs`/any active `subscribe_queue` from scratch** — this is
the subscriber's responsibility, not something `connection` can or should do on its behalf, since
`connection` has no knowledge of `transport:2`-specific subscription state (nor should it, per CLAUDE.md
§1's ownership boundary). Whatever seam Phase 1 designs on `ConnectionHandle` should make "the old
request/subscription state is gone, start over" the natural behavior after a reconnect signal, rather than
something each subscriber has to detect indirectly.

## Control verbs

All take a body, get back a single `COMPLETE` (`Success`, or an error name, no `CONTINUE`s) — plain
one-shot RPCs, not subscriptions. `zone_or_output_id` accepts either a `zone_id` or an `output_id`
(zone-level actions resolve either) — `output_id` alone is used where the JSDoc and both Rust ports agree
the action is inherently per-output (volume, mute, standby-family).

| Verb | Body fields | Notes |
|---|---|---|
| `control` | `zone_or_output_id`, `control` | `control ∈ {play, pause, playpause, stop, previous, next}`. Check the corresponding `is_*_allowed` on `Zone` before offering the control in UI. |
| `seek` | `zone_or_output_id`, `how ∈ {relative, absolute}`, `seconds` | |
| `change_volume` | `output_id`, `how ∈ {absolute, relative, relative_step}`, `value` | `value` is a **float** per the JSDoc's explicit note (see Rust divergence below) |
| `mute` | `output_id`, `how ∈ {mute, unmute}` | |
| `mute_all` | `how ∈ {mute, unmute}` | applies to every mutable zone, no target id |
| `pause_all` | *(none)* | |
| `standby` | `output_id`, `control_key?` | if `control_key` omitted, every standby-capable source control on the output is put into standby |
| `toggle_standby` | `output_id`, `control_key?` | |
| `convenience_switch` | `output_id`, `control_key?` | "take out of standby and select this source" in one call |
| `change_settings` | `zone_or_output_id`, plus any of `shuffle` (bool), `auto_radio` (bool), `loop` (`"loop"\|"loop_one"\|"disabled"\|"next"`) | only fields present in the body are changed; `loop: "next"` cycles the setting rather than setting an explicit value |
| `transfer_zone` | `from_zone_or_output_id`, `to_zone_or_output_id` | moves the queue from one zone to another |
| `play_from_here` | `zone_or_output_id`, `queue_item_id` | jump playback to a specific queue position |
| `get_zones` | *(none)* | one-shot `COMPLETE Success { zones: Zone[] }` — snapshot without subscribing |
| `get_outputs` | *(none)* | one-shot, `{ outputs: Output[] }` |

## Grouping verbs (shape only — grouping itself out of scope this phase)

Per IMPL_TRANSPORT.md's non-goals, grouping/ungrouping is deferred, but the wire shape is trivial and
already visible from this same study, so recorded here to avoid a second pass later:

- `group_outputs`: body `{ output_ids: string[] }` — combines the listed outputs into one zone; the first
  output's zone's queue is preserved.
- `ungroup_outputs`: body `{ output_ids: string[] }` — splits the listed outputs back out.

Both are one-shot `COMPLETE`-only requests, same shape as the control verbs above — no separate grouping
subscription channel exists; grouping changes surface through the ordinary `zones_added`/`zones_removed`
deltas on the existing `subscribe_zones` stream (a group/ungroup changes which `Zone` an `Output` belongs
to, which is exactly a zone add/remove/change from the subscriber's point of view).

## Rust design notes (for whoever implements `core::roon::transport`)

- Both Rust ports independently model `Zone`/`Output`/`Volume`/`NowPlaying` as typed structs matching the
  JSDoc field-for-field, reasonable corroboration that the JSDoc is accurate and complete for these types.
  `roon-rs` in particular already separates request-shaped "what we send" (its `ControlAction`/`SeekMode`/
  `VolumeMode`/`MuteAction` enums, matching CONVENTIONS.md's error-handling style of matchable typed
  values over raw strings) from response-shaped "what we parse" (`Zone`/`Output`/`ZoneSeek`), a split worth
  following here too.
- **`source_controls` cardinality divergence**: the JSDoc's `@property` block for `Output.source_controls`
  is written as a single object (`source_controls.display_name`, `source_controls.status`, ...), but the
  standby-family verbs' own `control_key` parameter only makes sense if an output can expose *more than
  one* source control, individually addressable. `TheAppgineer/rust-roon-api` models it as
  `Option<Vec<SourceControls>>` (array); `shin1ohno/roon-rs`'s `Output` struct omits the field entirely.
  Treat it as an array in `core::roon::transport` — the JSDoc's singular formatting looks like a docs
  artifact, not a real single-value constraint, and the array model is the only one consistent with
  `control_key` existing at all.
- **`change_volume` value type divergence**: the JSDoc is explicit that volume `value`/`min`/`max`/`step`
  are floating point, "not integers", and `roon-rs`'s `change_volume(&self, ..., value: f64)` matches that.
  `TheAppgineer/rust-roon-api`'s `change_volume(&self, ..., value: i32)` does not — likely a modeling gap
  in that port rather than a real protocol constraint, since it contradicts the primary source directly.
  Use a float type for volume values here.
- `TheAppgineer/rust-roon-api`'s `Volume` struct additionally carries `hard_limit_min`, `hard_limit_max`,
  `soft_limit` fields with no corresponding JSDoc mention in either source repo. Not contradicted by
  anything else read, just unconfirmed by the primary source — worth treating as present-but-unverified
  rather than baking into a model with confidence equal to the JSDoc-backed fields.
- Subscription-key allocation is an implementation choice, not a protocol requirement: the JS reference
  auto-increments a per-connection counter (any `subscription_key` the subscriber picks is accepted, the
  Core just echoes it back to disambiguate `CONTINUE`s when a client has several subscriptions of the same
  kind open at once — which, per this app's single-zone/single-Core scope, it never will). `roon-rs`'s
  example hardcodes `0`/`1` for its one zones-subscription and one outputs-subscription; that's fine for an
  app that only ever opens one of each, which matches this project's scope (CLAUDE.md's multi-zone/
  multi-Core non-goal).

## What this study deliberately leaves open

- **Full property enumeration**: as with sood-moo.md's SOOD properties, only fields actually referenced by
  the JSDoc and both Rust ports are documented above. Response bodies are JSON objects; an implementation
  can parse and retain unknown fields (`serde(flatten)` into a catch-all, or simply ignore unknown keys
  with `serde`'s default behavior) without needing this document extended first.
- **Grouping semantics beyond the wire shape** (which output becomes "primary" beyond "first output's
  queue is preserved", volume-linking behavior across a group, etc.) — deferred to whatever phase actually
  implements grouping, per IMPL_TRANSPORT.md's non-goals.
- **Error/status name enumeration**: `COMPLETE` names observed across the sources include `Success`,
  `Subscribed`, `Unsubscribed`, `Changed`, and sood-moo.md's already-documented `InvalidRequest` — no
  source enumerates a complete list of every possible non-success status a given verb can return (e.g.
  what a `control` call against a zone with `is_play_allowed: false` actually replies with). Left for
  Phase 3 to discover empirically against a real Core if it matters for error handling there, consistent
  with sood-moo.md's own "not safely re-verifiable without a live Core in the loop" lesson from
  `docs/IMPL_CORE_CONNECTION.md`'s Phase 4.1 finding — no automated request/response probing against a real
  Core planned here either.
- **`browse:1`, `image:1`** — separate services, explicitly out of scope for this study per
  IMPL_TRANSPORT.md's non-goals; `now_playing.image_key`/`QueueItem.image_key` are opaque keys meant to be
  resolved through `image:1`, not interpreted here.
