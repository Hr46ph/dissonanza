# `com.roonlabs.browse:1` wire-protocol study

Status: study complete, unimplemented. Written to unblock `core::roon::browse` (IMPL_BROWSE.md Phase 0) —
before this existed, the module's own design (in particular, how much per-session browse-stack state to
track internally) couldn't be decided without guessing at message shapes, the same reasoning
[sood-moo.md](sood-moo.md) and [transport.md](transport.md) document for their own modules. Nothing in
this document is copied verbatim from the sources below; it's an independent description of the wire
format, written from reading and cross-checking them.

**Scope note**: like `transport:2`, `browse:1` is a service *the Core provides* — this extension is the
client. It must be declared in `required_services`/`optional_services` at registration
(`moo::handshake::register`), and the extension then sends `REQUEST`s against it and receives a single
`COMPLETE` response each — see "Request/response verbs" below. Unlike `transport:2`'s `subscribe_zones`,
there is no subscription/`CONTINUE` stream anywhere in this service: every `browse`/`load` call is a
plain one-shot RPC.

## Sources studied

| Source | What it is | License | Role here |
|---|---|---|---|
| [`RoonLabs/node-roon-api-browse`](https://github.com/RoonLabs/node-roon-api-browse) | Official JS SDK's browse module, published by Roon Labs | Apache-2.0 | **Primary/normative** — `lib.js` read in full |
| [`shin1ohno/roon-rs`](https://github.com/shin1ohno/roon-rs) | Community Rust SDK (`roon-api` crate on crates.io) | MIT OR Apache-2.0 | Corroboration + Rust typed-model reference — `crates/roon-api/src/browse.rs` |
| [`TheAppgineer/rust-roon-api`](https://github.com/TheAppgineer/rust-roon-api) | Community Rust SDK (git-only, alpha) | MIT | Secondary corroboration, including its worked example — `src/browse.rs` |

Note: `node-roon-api-browse` is a separate repo from `node-roon-api` (the one sood-moo.md studied), same
pattern `node-roon-api-transport` followed — Roon Labs ships each service module as its own npm package
layered on the core SDK. All three sources agree on every request/response shape below; one divergence
(a scope limitation, not a protocol disagreement) is called out in "Rust design notes".

## Service identity

`SVCNAME = "com.roonlabs.browse:1"`. Every request name below is `SVCNAME + "/" + <method>`, e.g.
`com.roonlabs.browse:1/browse`, sent as a MOO `REQUEST` per sood-moo.md's framing. All bodies are JSON
(`Content-Type: application/json`).

## Data model

**`List`** (one level of the browse stack):

| Field | Type | Notes |
|---|---|---|
| `title` | string | |
| `count` | int | number of items in this level |
| `subtitle` | string, optional | |
| `image_key` | string, optional | opaque key, resolved through `image:1` (out of scope here, same as `transport:2`'s `image_key` fields) |
| `level` | int | increases from 0 as the user drills down |
| `display_offset` | int, optional | the stored scroll/paging position for this list |
| `hint` | `null \| "action_list"`, optional | an unrecognized value must be treated as `null` — the JSDoc explicitly asks implementations to tolerate future hint values |

**`Item`** (one entry within a `List`):

| Field | Type | Notes |
|---|---|---|
| `title` | string | |
| `subtitle` | string, optional | |
| `image_key` | string, optional | |
| `item_key` | string, optional | pass this into the next `browse` call when the user selects this item |
| `hint` | `null \| "action" \| "action_list" \| "list" \| "header"`, optional | same forward-compatibility note as `List.hint`: an unrecognized value must be treated as `null`. `"header"` is display-only, no click action |
| `input_prompt` | object, optional | present only when selecting this item requires free-text input first (e.g. a search box) |

`input_prompt`: `{ prompt, action, value?, is_password }` — `prompt` is the label to show (e.g. "Search
Albums"), `action` is the button verb (e.g. "Go"), `value` pre-populates the input if present,
`is_password` requests a masked input field.

## Session/browse-stack semantics

Unlike `transport:2`'s subscriptions, **browse state lives on the Core, not the client** — the JSDoc is
explicit about this being the whole point of the service ("Your browsing session is maintained on Roon's
side, facilitating minimally stateful clients"). The client doesn't hold a browse stack itself; it holds
at most the `hierarchy` string and an optional `multi_session_key`, and the Core tracks everything else
(current level, item selection history, scroll position) keyed by those two values plus the paired
connection's identity.

- **`hierarchy`**: identifies which navigation tree is being browsed. Documented values: `"browse"`
  (general-purpose top-level browser), `"playlists"`, `"settings"`, `"internet_radio"`, `"albums"`,
  `"artists"`, `"genres"`, `"composers"`, `"search"`. Required on every `browse`/`load` call.
- **`multi_session_key`**: optional; lets one extension keep more than one independent browse position
  open at once within the same `hierarchy` (e.g. a sidebar browser and a search box, browsed
  concurrently). Most callers omit it (a single implicit session per hierarchy). Neither `browse`'s nor
  `load`'s *response* body echoes `multi_session_key` back — it is purely an outbound request field, so a
  caller juggling more than one session must correlate the response with the request that produced it
  itself (see "Rust design notes" below for how `TheAppgineer/rust-roon-api` does this, and why our own
  design doesn't need the same mechanism).
- **The browse stack**: levels are numbered from 0 upward as the user drills in. `opts.pop_all` (bool)
  resets to the root; `opts.pop_levels` (int) pops back a specific number of levels; `opts.refresh_list`
  (bool) re-fetches the current level's contents without changing position; `opts.item_key` descends into
  the selected item (omitting it just reloads the current level). `pop_all` and `item_key` are mutually
  exclusive on one call per the JSDoc.
- **Reconnect implication — left unconfirmed by every source read**: none of `lib.js`, `roon-rs`, or
  `rust-roon-api` say whether a Core-side browse-stack/session survives a disconnect (a new MOO
  registration after `connection`'s `Discovering → ... → Paired` loop, per CLAUDE.md §1's reconnect
  behavior). `transport.md`'s equivalent finding for `subscribe_zones` was possible because `moo.js`'s
  `clean_up`/`Moo` lifecycle is explicit that subscription state resets to nothing on a new connection;
  no equivalent statement exists anywhere in the browse-specific sources for the *Core*-side session
  state this service is built around. Flagged as unconfirmed rather than guessed — see "What this study
  deliberately leaves open" below. What *is* certain (from `transport.md`'s already-documented `moo.js`
  behavior, which is generic to all MOO requests, not `transport:2`-specific): any `browse`/`load` request
  in flight at the moment of a disconnect has its response callback fired with no arguments, i.e. its
  `MooResponseStream` simply ends — the same behavior every other `core::roon` module built on
  `ConnectionRequests` already relies on, nothing browse-specific to add there.

## Request/response verbs

Both are one-shot requests: a single `REQUEST`, a single `COMPLETE` response (`Success`, or an error
name — see below), no `CONTINUE`s.

| Verb | Body fields | Notes |
|---|---|---|
| `browse` | `hierarchy`, `multi_session_key?`, `item_key?`, `input?`, `zone_or_output_id?`, `pop_all?`, `pop_levels?`, `refresh_list?`, `set_display_offset?` | Use when the user selects an `Item`, submits an `input_prompt`, or navigates the stack (`pop_all`/`pop_levels`/`refresh_list`). `zone_or_output_id` is required for any playback-related action the hierarchy exposes (e.g. selecting "Play Now" inside `"browse"`) but is otherwise unused. Response body: `{ action, list?, item?, message?, is_error? }` — see below for `action`'s possible values. |
| `load` | `hierarchy`, `multi_session_key?`, `level?`, `offset?`, `count?`, `set_display_offset?` | Retrieves items from a level separately from navigating to it, so large lists can be paged in small increments. `level` defaults to the current (deepest) level, `offset` defaults to 0, `count` defaults to 100. Response body: `{ items: Item[], offset, list: List }` — always a plain snapshot, no delta/paging cursor beyond the `offset`/`count` the caller itself tracked. |

**`browse` response `action` values** (JSDoc-documented, all four have a distinct, mutually exclusive
meaning — no `CONTINUE`-style incremental variant of any of them):

| `action` | Meaning | Relevant body fields |
|---|---|---|
| `"list"` | The current list (or its contents) changed — call `load` next to fetch its items | `list` |
| `"message"` | Show a message to the user (e.g. "No results found") | `message`, `is_error` |
| `"none"` | No further action required | *(none)* |
| `"replace_item"` | Replace the selected item in place with the one given | `item` |
| `"remove_item"` | Remove the selected item from the current list | *(none)* |

Both `browse` and `load`'s callback signature in the reference SDK reports an error as `msg.name` when
the response isn't `Success` (falling back to a synthetic `"NetworkError"` if the connection dropped with
no response at all) — the same `COMPLETE`-name-as-error-signal pattern `transport.md`'s control verbs
already documented, not a new mechanism.

## Rust design notes (for whoever implements `core::roon::browse`)

- Both Rust ports independently model `BrowseOptions`/`LoadOptions` (request) separately from
  `BrowseResult`/`LoadResult`/`List`/`Item` (response) — the same "what we send" vs. "what we parse" split
  `transport.md`'s own Rust design notes recommended following, already adopted by `transport::control`.
- **Hierarchy scope divergence**: `shin1ohno/roon-rs`'s `BrowseOptions.hierarchy` is a free `Option<String>`
  the caller sets to any of the documented values, matching the primary source's full multi-hierarchy
  design. `TheAppgineer/rust-roon-api`'s `BrowseOpts` has *no* `hierarchy` field at all — `browse()`/
  `load()` hardcode `opts["hierarchy"] = "browse"` internally, permanently restricting that port to the
  general-purpose browser and making `"playlists"`/`"settings"`/`"albums"`/etc. unreachable through it.
  This reads as a scope limitation in that port, not evidence the protocol only really has one hierarchy —
  the primary JSDoc lists eight values and `roon-rs` corroborates all of them being caller-supplied.
  `core::roon::browse` should take `hierarchy` as a real parameter, not hardcode one.
- **`multi_session_key` correlation**: because neither `browse` nor `load`'s response body echoes
  `multi_session_key` back, `TheAppgineer/rust-roon-api` keeps its own `HashMap<request_id,
  multi_session_key>` (`Browse::session_keys`) so its event-based `parse_msg` can re-attach the session key
  to whichever response arrives. `core::roon::browse` doesn't need an equivalent table: because it will be
  built on `ConnectionRequests::send_request` (an `async fn` returning a `MooResponseStream` scoped to that
  one call, exactly like `transport::control`'s `control`/`seek`), the caller already knows which
  `hierarchy`/`multi_session_key` it used at the same call site where it awaits the response — there is no
  broadcast/dispatch step in between that needs help re-associating the two. This is a real simplification
  from the JS/TheAppgineer callback-registry model, not something to replicate.
- **Error-name enumeration**: the primary JSDoc only specifies the *shape* (`string | false`), not the
  actual error names. `TheAppgineer/rust-roon-api`'s `parse_msg` match arm is the only source observed to
  enumerate concrete ones: `"InvalidItemKey"`, `"InvalidLevels"`, `"UnexpectedError"`, `"ZoneNotFound"`.
  Treat these as corroborated-by-one-secondary-source, not primary-confirmed — worth modeling (mirroring
  `transport::error::TransportError::CommandFailed`'s generic "any other `COMPLETE` name" fallback rather
  than an exhaustive enum) so an unrecognized name doesn't become a parse failure.
- Both `List.hint`/`Item.hint` are modeled as enums in both Rust ports (`shin1ohno` leaves them as
  `Option<String>`, less strict; `TheAppgineer` defines `ListHint`/`ItemHint` enums including a literal
  `#[serde(rename = "null")]` variant for the "unrecognized/no hint" case). The JSDoc's own
  forward-compatibility instruction ("if you see a hint you do not recognize, treat it as `null`") argues
  for a permissive parse (an unknown string maps to the "no hint" case rather than a hard parse error),
  regardless of which shape is chosen.

## What this study deliberately leaves open

- **Session persistence across a `connection` reconnect**: flagged above under "Session/browse-stack
  semantics" — no source read states whether the Core retains a `hierarchy`/`multi_session_key`'s stack
  position across a new MOO registration. A future phase's design should either treat every post-reconnect
  `browse` call defensively (e.g. `pop_all: true` if position doesn't matter, or verify a stale position
  still resolves) or note this as a real user-visible gap if it doesn't — not something to resolve by
  probing a real Core, consistent with `docs/IMPL_CORE_CONNECTION.md` Phase 4.1's "not safely re-verifiable
  without a live Core in the loop" finding, the same lesson `transport.md` already deferred its own
  error-name enumeration behind.
- **Full error-name enumeration**: only four names are corroborated (by one secondary source, not the
  primary JSDoc) — see "Rust design notes" above.
- **Per-hierarchy behavioral nuances**: whether `"settings"`, `"internet_radio"`, etc. have any
  hierarchy-specific quirks beyond the generic `browse`/`load` envelope (e.g. does `"search"` require
  `input` on the very first call, or can it be reached via `item_key` first) isn't documented by any
  source read — left for empirical discovery once a real Core is in the loop, same caveat as above.
- **`image:1`** — `List.image_key`/`Item.image_key` are opaque keys meant to be resolved through `image:1`,
  a separate future phase, not interpreted here — same scope boundary `transport.md` already drew for
  `now_playing.image_key`.
