# CURRENT_STATE.md

Living project map. Update when the project's structure, capabilities, or known open work materially changes — it's the fastest way for a new session (human or agent) to get oriented without re-reading the whole codebase. Not an obligation to log every implementation detail: a renamed variable or a one-line tweak isn't material, a new module or a capability going from "planned" to "done" is.

Keep entries short. This file describes *what is*, not *how it should be built* (that's [CONVENTIONS.md](CONVENTIONS.md)/[TECH_STACK.md](TECH_STACK.md)) or *where it's headed* (that's [NORTH-STAR.md](NORTH-STAR.md)).

## File Structure

Cargo workspace with two crates (`core/`, `app/`) plus the documentation/scaffolding from before:

```
dissonanza/
├── Cargo.toml           # workspace root: members = ["core", "app"], shared version/edition
├── CLAUDE.md            # agent operating instructions
├── cliff.toml           # git-cliff config: generates CHANGELOG.md from Conventional Commits
├── CONTEXT.md           # debugging memory, written only on explicit instruction
├── CONVENTIONS.md       # engineering conventions
├── COPY.md              # tone/voice/terminology
├── CURRENT_STATE.md     # this file
├── DESIGN.md            # visual/UX spec
├── NORTH-STAR.md        # long-term vision
├── RELEASES.md          # versioning/release/CI/packaging process
├── TECH_STACK.md        # chosen stack
├── core/                # `dissonanza-core` crate — Roon protocol, business logic, no GUI deps
│   └── src/
│       ├── lib.rs           # pub mod roon;
│       └── roon/
│           ├── mod.rs           # pub mod connection;
│           └── connection/      # sole owner of Core discovery/pairing/keepalive (CLAUDE.md §1)
│               ├── mod.rs           # PUBLIC surface: Connection, ConnectionHandle, ConnectionRequests,
│               │                    #   ConnectionConfig, ConnectionEvent, ConnectionState, ConnectionError
│               │                    #   (+ DiscoveryError/TransportError/HandshakeError re-exports),
│               │                    #   MooMessage/MooVerb/MooBody re-exports. sood/moo stay private.
│               ├── config.rs        # ConnectionConfig — extension identity for MOO registration
│               ├── state.rs         # ConnectionState, ConnectionEvent
│               ├── error.rs         # ConnectionError — wraps DiscoveryError/TransportError/HandshakeError
│               ├── keepalive.rs     # Keepalive — app-level staleness health-check
│               ├── requests.rs      # ConnectionRequests/MooResponseStream/ConnectionRequestError — generic
│               │                    #   MOO request/response multiplexing for other core::roon modules
│               ├── sood/            # private: SOOD discovery, never `pub` outside `connection`
│               │   ├── mod.rs
│               │   ├── message.rs   # SoodMessage/SoodMessageType/SoodError — TLV codec, pure parsing
│               │   └── discovery.rs # per-interface multicast sockets, query cadence, dedupe by unique_id
│               └── moo/             # private: MOO websocket protocol, never `pub` outside `connection`
│                   ├── mod.rs
│                   ├── message.rs   # MooMessage/MooVerb/MooBody/MooError — message framing, pure parsing
│                   ├── transport.rs # websocket connect, MOO frame send/receive, WS ping/pong keepalive
│                   └── handshake.rs # registry:1/info + /register handshake; pairing:1 + ping:1 service responders
│           └── transport/       # com.roonlabs.transport:2 client — Core-provided, this extension consumes it
│               ├── mod.rs           # PUBLIC surface: model types, TransportError, subscribe_zones/ZoneEvent/
│               │                    #   ZoneSeekChange/ZoneSubscription, control/seek/ControlAction/SeekHow,
│               │                    #   change_volume/mute/standby/ChangeVolumeHow/MuteHow
│               ├── model.rs         # Zone/Output/ZoneState/ZoneSettings/LoopMode/NowPlaying/OneLine/TwoLine/
│               │                    #   ThreeLine/SourceControl/SourceControlStatus/Volume/VolumeType — pure
│               │                    #   serde::Deserialize types, no I/O
│               ├── error.rs         # TransportError — this module's own aggregate error type
│               ├── zones.rs         # subscribe_zones, ZoneEvent/ZoneSeekChange, ZoneSubscription — built on
│               │                    #   connection::ConnectionRequests, never touches SOOD/pairing/reconnect
│               └── control.rs       # control/seek/change_volume/mute/standby one-shot verbs,
│                                    #   ControlAction/SeekHow/ChangeVolumeHow/MuteHow — same
│                                    #   ConnectionRequests-only, no SOOD/pairing/reconnect pattern as zones.rs
├── app/                  # `dissonanza` crate (binary) — Slint UI shell, depends on core's public API
│   └── src/
│       └── main.rs           # trivial placeholder, no Slint wired up yet
├── .github/
│   └── workflows/
│       ├── ci.yml       # format/lint/test on every push/PR, version-bump guardrail on PRs into main
│       └── publish.yml  # binary build, Flatpak validation, GitHub Release, AUR push on tag
├── packaging/
│   ├── flatpak/
│   │   ├── io.github.hr46ph.Dissonanza.yml            # Flatpak manifest (draft, see RELEASES.md gaps)
│   │   ├── io.github.hr46ph.Dissonanza.desktop         # desktop entry
│   │   └── io.github.hr46ph.Dissonanza.metainfo.xml    # AppStream metadata (draft)
│   └── aur/
│       └── PKGBUILD    # AUR package recipe (draft, checksums pending a real tag)
├── docs/
│   ├── roon-linux-remote-client-onderzoek.md   # original research notes (Dutch); source for the files above
│   ├── IMPL_CORE_CONNECTION.md   # completed implementation plan for core::roon::connection (Phases 0-4)
│   ├── IMPL_TRANSPORT.md   # completed implementation plan for core::roon::transport (Phases 0-5)
│   └── protocol/
│       ├── sood-moo.md    # SOOD/MOO wire-protocol study — normative reference for core::roon::connection
│       └── transport.md   # com.roonlabs.transport:2 wire-protocol study — normative reference for
│                           #   core::roon::transport (implemented, see Modules below)
└── README.md
```

## Modules

- **`core::roon::connection`** (`core/src/roon/connection/`) — per CLAUDE.md §1, the sole owner of
  Core discovery, `core_paired`/`core_unpaired` handling, keepalive, and reconnect. Has a
  public API: `Connection::spawn(ConnectionConfig) -> (ConnectionHandle, ConnectionRequests,
  mpsc::UnboundedReceiver<ConnectionEvent>)` wires discovery → MOO connect → registry handshake →
  the pairing:1/ping:1 request loop → keepalive into one task, looping back to a fresh
  `Discovering` pass on any disconnect (transport closed, keepalive stale, a step failed) until
  `ConnectionHandle::shutdown` is called — SOOD discovery starts over from scratch every time, so
  a Core's address is never redialed, per CLAUDE.md's mandatory technical choices. `sood`/`moo`
  stay private modules (only reachable from `connection` itself, via `pub(super)`) — nothing
  outside this module calls them directly.
  - `connection::requests` — `ConnectionRequests`: a `Clone`-able handle (IMPL_TRANSPORT.md Phase
    1, done) other `core::roon` modules use to send their own MOO requests
    (`send_request(name, body) -> Result<MooResponseStream, ConnectionRequestError>`) over
    whichever connection is currently past the registry handshake, without `connection` needing to
    know about `transport:2`/`browse:1`/etc. specifically. Backed by a command channel into
    `run_until_disconnected`'s request loop (a fresh dispatch table + command channel per
    connection attempt, request-ids allocated from `3` since `handshake::register` reserves `1`
    and `2`), published through a `watch` cell that's `None` whenever no connection is up.
    `MooResponseStream` yields the `CONTINUE`/`COMPLETE`s for one request and ends (`recv` returns
    `None`) on `COMPLETE` or on disconnect alike — a caller with an open subscription sees it end
    exactly like a finished one-shot request, so re-issuing it after the next `Paired` is always
    the caller's own job, never automatic, matching `docs/protocol/transport.md`'s reconnect
    finding. Cleanup of an abandoned stream (dropped without unsubscribing) is lazy: reaped the
    next time a message for it fails to forward, not proactively on drop — a flagged, minor known
    gap, not built out further until shown to matter. `MooMessage`/`MooVerb`/`MooBody` are
    re-exported from `connection` (promoted from private) as this surface's response type, rather
    than a parallel public type.
  - `connection::state` — `ConnectionState` (`Discovering`, `Connecting`, `Registering`,
    `Paired { core_id }`, `Disconnected`) and `ConnectionEvent` (`StateChanged`, `Error`), emitted
    on `Connection::spawn`'s event channel.
  - `connection::error` — `ConnectionError`, wrapping `DiscoveryError`/`TransportError`/
    `HandshakeError` (re-exported from `connection` so callers can match on them without reaching
    into `sood`/`moo`).
  - `connection::config` — `ConnectionConfig` (`extension_id`, `display_name`, `display_version`,
    `publisher`, `email`, optional `website`): the extension identity `moo::handshake::register`
    sends during registration. Now `pub` (it's `Connection::spawn`'s input).
  - `connection::keepalive` — `Keepalive`: tracks when activity (any inbound MOO message) was
    last observed and reports the connection stale once `timeout` passes with none, regardless of
    whether `core_paired`/`core_unpaired` fired. Wired into `connection/mod.rs`'s request loop
    with a judgment-call default (60s timeout, checked every 10s) — `docs/protocol/sood-moo.md`
    doesn't document a Core-side app-level ping cadence to derive this from, flagged as an
    explicit assumption rather than a sourced value.
  - `sood::message` — SOOD TLV packet parsing/encoding (`SoodMessage`, `SoodError`). Pure, no I/O.
  - `sood::discovery` — the multicast discovery loop: one send/receive socket per local IPv4
    interface (`socket2`), 5s interface re-enumeration (`if-addrs`), query cadence (10s×6 then 60s),
    dedupe by `unique_id`, `_replyaddr`/`_replyport` override. `connection/mod.rs` takes the first
    discovered Core and stops discovery once it has one — it doesn't yet keep discovery running as
    a fallback in case that candidate fails, or retry; that's reconnect-on-disconnect territory,
    still open below.
  - `moo::message` — MOO message framing (`MooMessage`, `MooVerb`, `MooBody`, `MooError`).
    Parses/encodes the header-block + blank-line + body wire format: `Request-Id` extraction,
    `Content-Length`/`Content-Type` cross-validation, JSON vs. raw-bytes body handling. Pure,
    no I/O.
  - `moo::transport` — the MOO websocket transport (`tokio-tungstenite`): connects to
    `ws://<addr>/api`, encodes/sends outbound `MooMessage`s and decodes/forwards inbound ones as
    binary WS frames over `mpsc` channels, and runs an application-level WS ping every
    (caller-supplied) interval, closing the connection if a pong is missed. A framing violation
    (malformed MOO bytes, or a text frame) ends the loop immediately rather than resyncing.
  - `moo::handshake` — the registry registration handshake: sends `registry:1/info` then
    `registry:1/register` (declaring caller-supplied `provided_services`, plus a saved token if
    the caller has one), and parses the `COMPLETE Registered` body (`core_id`, `token`,
    `display_name`, `display_version`, `provided_services`) into a typed `Registered`. Also
    implements the `com.roonlabs.pairing:1` service this extension provides in return
    (`PairingState`/`PairingEvent`): `subscribe_pairing`/`unsubscribe_pairing`/`get_pairing`
    report current pairing status, and an inbound `pair` request (the Core, when the user pairs
    this extension in Roon's UI) sets it and emits `PairingEvent::Paired`. There is no `unpair`
    wire message — per `node-roon-api`, unpairing is inferred purely from the moo connection
    closing, so it's handled where connection lifecycle is tracked (the keepalive backstop
    above), not here. Also implements the `com.roonlabs.ping:1` service this extension
    provides in return (`handle_ping_request`, stateless): replies `COMPLETE Success` to an
    inbound `ping` request, distinct from the WS-level ping/pong `moo::transport` already runs.
    Operates purely over `mpsc` channels shaped like `moo::transport`'s, so it's tested without a
    real websocket.

- **`core::roon::transport`** (`core/src/roon/transport/`) — the `com.roonlabs.transport:2` client
  (IMPL_TRANSPORT.md Phase 2, done): zone subscription and the typed data model, built entirely on
  `connection::ConnectionRequests` — never touches SOOD discovery, pairing, or reconnect itself,
  per CLAUDE.md §1. `connection`'s `REQUIRED_SERVICES` now declares
  `"com.roonlabs.transport:2"` (a literal there, not a constant imported from this module, so
  `connection` stays ignorant of `transport:2` specifically beyond needing its name to register).
  - `transport::model` — `Zone`/`Output`/`ZoneState`/`ZoneSettings`/`LoopMode`/`NowPlaying`/
    `OneLine`/`TwoLine`/`ThreeLine`/`SourceControl`/`SourceControlStatus`/`Volume`/`VolumeType`:
    pure `serde::Deserialize` types matching docs/protocol/transport.md's data model field-for-
    field, including its two flagged divergences (`source_controls` as an array; volume
    `min`/`max`/`value`/`step` as floats). `Volume`'s unverified `hard_limit_min`/
    `hard_limit_max`/`soft_limit` fields (seen in only one community port) are deliberately left
    out for now — `serde` ignores unknown fields, so adding them later costs nothing.
  - `transport::error` — `TransportError`, this module's own aggregate error type (parallel to
    `connection::error::ConnectionError`). Shares a name with, but is a distinct type from,
    `connection::TransportError` (the MOO *websocket* transport's error) — code importing both
    needs to alias one on `use`, flagged rather than resolved since renaming either would break
    the "each module's error is named after its own domain" convention `HandshakeError`/
    `DiscoveryError` already established.
  - `transport::zones` — `subscribe_zones(&ConnectionRequests) -> Result<ZoneSubscription,
    TransportError>` sends the request with a hardcoded `subscription_key: 0` (this app only ever
    opens one zones subscription, per CLAUDE.md's multi-zone/multi-Core non-goal, so nothing needs
    allocating); `ZoneSubscription::recv(&mut self) -> Option<Result<ZoneEvent, TransportError>>`
    parses each `CONTINUE` into `ZoneEvent::Subscribed { zones }` or `ZoneEvent::Changed {
    zones_added, zones_changed, zones_removed, zones_seek_changed }`. A malformed body or an
    unexpected verb/name surfaces once as an `Err` and then ends the subscription for good (no
    resync attempt), mirroring `moo::transport`'s own framing-violation handling — further `recv`
    calls return `None`. No `unsubscribe_zones` this phase: dropping the `ZoneSubscription` is the
    only way to end interest early, and `connection`'s dispatch-table cleanup already tolerates
    that lazily. Re-issuing `subscribe_zones` after a reconnect (`ZoneSubscription::recv`
    returning `None` covers both "Core ended it" and "connection was lost" identically) is left
    entirely to whatever future caller owns that decision — this module never loops or retries on
    its own. `connection::requests::MooResponseStream` gained a `pub(crate) fn new(rx) -> Self`
    (previously only constructed inline inside `send_request`) purely so this module's tests can
    fabricate a response stream over a plain `mpsc` channel, the same "unit-tested purely over
    channels, no live Core" pattern `moo::handshake` and `connection::requests` already use;
    `subscribe_zones` itself (a two-line wrapper) isn't separately tested beyond type-checking,
    since the logic it delegates to is already covered by `requests.rs`'s own tests.
  - `transport::control` (IMPL_TRANSPORT.md Phase 3, done) — `control(&ConnectionRequests,
    zone_or_output_id, ControlAction) -> Result<(), TransportError>` and `seek(&ConnectionRequests,
    zone_or_output_id, SeekHow, seconds) -> Result<(), TransportError>`: one-shot playback verbs,
    typed `ControlAction`/`SeekHow` enums serializing to the wire's `control`/`how` string values
    (`PlayPause` needs an explicit `#[serde(rename = "playpause")]` — snake_case alone would
    produce `play_pause`, which the Core doesn't accept). Both wait for the single `COMPLETE` these
    verbs reply with via a shared `await_command_response`/`parse_command_response` pair (the
    latter pure, unit-tested directly against fabricated `MooMessage`s, mirroring `zones.rs`'s
    `parse_zone_event` split): `COMPLETE Success` → `Ok(())`, any other `COMPLETE` name → a new
    `TransportError::CommandFailed { name }` (no source enumerates every non-success status a verb
    can return, so any other name is treated generically), anything other than a `COMPLETE` →
    the existing `TransportError::UnexpectedResponse` (its message text generalized off "for a zone
    subscription" now that `control`/`seek` share it too), stream-ends-with-nothing → a new
    `TransportError::NoResponse { name }`. No subscription/reconnect handling needed — these are
    one-shot requests, not subscriptions. `control`/`seek` themselves aren't separately tested
    beyond type-checking plus a couple of enum-serialization-shape assertions, same precedent
    `subscribe_zones` set. Phase 4 (done, same module) added `change_volume(&ConnectionRequests,
    output_id, ChangeVolumeHow, value: f64) -> Result<(), TransportError>` (`value` is a float per
    the wire study, not the `i32` one community Rust port uses), `mute(&ConnectionRequests,
    output_id, MuteHow)`, and `standby(&ConnectionRequests, output_id, control_key: Option<&str>)`
    — all output-scoped, all thin wrappers reusing `await_command_response`/`parse_command_response`
    unchanged, no new `TransportError` variants needed since Phase 3 already generalized
    `UnexpectedResponse` for this reuse.

## Open work

- `core::roon::connection` implementation in progress on `feature/roon-connection-core` (branched from
  a new `develop`, per CLAUDE.md's git workflow): SOOD TLV parsing, the SOOD multicast discovery
  loop, MOO message framing, the MOO websocket transport, the MOO registry registration
  handshake, the inbound `com.roonlabs.pairing:1`/`com.roonlabs.ping:1` services, the app-level
  keepalive staleness check, the connection state machine and public `Connection` API wiring all
  of it together, and now reconnect-on-disconnect are done (see Modules above) — Phase 3 is
  complete. Phase 4.1's originally-planned automated end-to-end integration test (fake SOOD
  responder + mock MOO/WS server driving a real `Connection` through `Discovering → Connecting →
  Registering → Paired`) was attempted and found **not safely runnable** — see the dated entry
  below for the full finding. **Resolved by user decision**: unit-test-only coverage (the existing
  49 per-module unit tests plus manual verification against a real Core) is accepted as final for
  this module — no further automated E2E test is planned. Phase 4 — and with it, this entire
  implementation plan (`IMPL_CORE_CONNECTION.md`) — is complete as of the
  `feature/roon-connection-core` → `develop` merge (4.2) recorded below. A real test seam for
  `connection` remains a possible, undecided future improvement, not committed work. Also
  open: pairing-token persistence, reconnect has no backoff yet (a disconnect that fails
  immediately and repeatedly — e.g. interface enumeration erroring on every attempt — loops back
  to `Discovering` with no delay; not part of 3.3's scoped behavior, flagged here rather than
  silently added), and the discovery-keeps-running-as-a-fallback-while-paired question noted under
  `sood::discovery` above (distinct from reconnect-after-disconnect, which is now done).
  - ~~Custom Rust SOOD/MOO protocol implementation needs its own wire-protocol study~~ — **done**, see
    [docs/protocol/sood-moo.md](docs/protocol/sood-moo.md): packet/message formats, the
    connection/registration/pairing handshake, and the keepalive rationale behind CLAUDE.md §1,
    cross-checked against the official `node-roon-api` SDK (Apache-2.0) and two MIT-licensed community
    Rust ports (`shin1ohno/roon-rs`, `TheAppgineer/rust-roon-api` — legally-clear reference/reuse
    material; "not relying on it" was about not taking a dependency, not about the code being
    off-limits). Still open: per-service (transport/browse/image/...) message body shapes aren't
    covered by this study — each needs its own; `transport:2`'s is now done, see the
    `core::roon::transport` entry below, `browse:1`/`image:1` remain open.
- `core::roon::transport` (`com.roonlabs.transport:2` — zone list, now-playing, playback control): all
  five phases of [docs/IMPL_TRANSPORT.md](docs/IMPL_TRANSPORT.md) are complete as of the
  `feature/roon-transport` → `develop` merge recorded below — see the `core::roon::transport` and
  `core::roon::connection` entries above for what each phase built. All four `cargo` gates green (81
  unit tests, `clippy -- -D warnings`, `fmt --check` all clean). Non-goals for this phase: zone
  grouping/ungrouping (wire shape documented anyway in the Phase 0 study, implementation deferred),
  `browse:1`/`image:1`, Slint UI, multi-zone/multi-Core (permanent, per NORTH-STAR.md).
- `core::roon::browse` (`com.roonlabs.browse:1` — sidebar categories, browse paths, search): planned in
  [IMPL_BROWSE.md](IMPL_BROWSE.md), chosen per user decision (2026-09-07) as the next vertical slice after
  `transport`, the other major leg of NORTH-STAR.md's "every sidebar category, browse path" parity goal.
  Phase 0 (wire-protocol study) is **done**, see [docs/protocol/browse.md](docs/protocol/browse.md) — it
  resolved the plan's open architectural question (no internal session/paging state needed;
  `browse`/`load` are stateless request/response pairs from this module's point of view, the actual
  browse-stack state lives Core-side). A Phase 1+ design sketch is written into IMPL_BROWSE.md but not
  yet confirmed with the user or split into numbered Gate-1 steps — no code changes yet.
- Slint GUI: not started (app/src/main.rs is a trivial placeholder).
- Pairing-token persistence (so a paired extension doesn't have to re-pair on every restart) is
  deferred until a cache-store phase exists — the MOO handshake step will hold it in memory only.
- Cache invalidation strategy for locally cached album art (when to refresh on a new Roon scan or changed art) undecided.
- DESIGN.md's color tokens, typography scale, and concrete component states are marked TBD pending user-supplied screenshots of Roon's official UI.
- Flatpak submission is blocked on making the repo public (currently private, see TECH_STACK.md) and on generating `cargo-sources.json` once real dependencies are locked in.
- AUR push in `publish.yml` is scaffolded but inert until `AUR_SSH_PRIVATE_KEY`/`AUR_USERNAME`/`AUR_EMAIL` secrets are configured, and until the package is claimed on AUR with a first manual push.
- `PKGBUILD` checksums are placeholders (`SKIP`) until a real `v0.1.0` tag exists.

## Recently changed

- Completed the `com.roonlabs.browse:1` wire-protocol study (2026-09-07):
  [docs/protocol/browse.md](docs/protocol/browse.md) — IMPL_BROWSE.md's Phase 0. Covers the `List`/`Item`/
  `InputPrompt` data model, the `browse`/`load` request/response envelope (both one-shot, single-`COMPLETE`
  RPCs — no subscription/`CONTINUE` stream anywhere in this service, unlike `transport:2`'s zones), and the
  `hierarchy`/`multi_session_key` session model. Sourced from `RoonLabs/node-roon-api-browse`'s `lib.js`
  (primary/normative) cross-checked against `shin1ohno/roon-rs` and `TheAppgineer/rust-roon-api`'s browse
  modules, same sourcing approach as `sood-moo.md`/`transport.md`. Key finding, resolving IMPL_BROWSE.md's
  open architectural question: because browse-stack state lives entirely on the Core (keyed by
  `hierarchy`/`multi_session_key`, not held by the client) and `core::roon::browse` will call
  `ConnectionRequests::send_request` directly per request (an `async fn` scoped to one call, unlike the JS
  SDK's callback-registry model), the module needs no internal session/paging state of its own — the
  caller already knows which session it used at the point it awaits the response. Flags one scope
  divergence (`TheAppgineer/rust-roon-api` hardcodes `hierarchy: "browse"` internally rather than exposing
  it as a parameter — a limitation in that port, not a protocol constraint) and one unconfirmed gap (no
  source states whether a Core-side browse-stack position survives a `connection` reconnect). No code
  changes; a Phase 1+ design sketch was added to IMPL_BROWSE.md but needs confirmation before being split
  into numbered Gate-1 steps.
- Drafted `IMPL_BROWSE.md` (2026-09-07): chosen per user decision as the next vertical slice —
  `com.roonlabs.browse:1`, over `image:1` and starting the Slint UI first — since `browse:1` is the other
  major leg of NORTH-STAR.md's parity goal alongside the now-complete `transport:2`, and a browse-capable
  Slint UI needs it before there's much to display. Only Phase 0 (the wire-protocol study, mirroring
  `sood-moo.md`/`transport.md`) is written and Gate-1 accepted; the module's own design (in particular,
  how much of `browse:1`'s per-session paging state to track internally) is flagged as an open question
  deferred until that study exists, per the same study-first precedent `IMPL_TRANSPORT.md` set. No code
  changes yet.
- Archived the completed transport implementation plan (2026-09-07): moved `IMPL_TRANSPORT.md` to
  [docs/IMPL_TRANSPORT.md](docs/IMPL_TRANSPORT.md) now that all five phases are done (see Open work
  above) — kept for reference, not deleted, same precedent as `IMPL_CORE_CONNECTION.md`'s archival.
  Never a tracked file (`/IMPL_*.md` and `/docs/*` are both gitignored, `/docs/protocol/` excepted),
  so this was a plain filesystem move, not a `git mv`.
- Added `core::roon::transport::control` for playback controls (2026-09-07): IMPL_TRANSPORT.md Phase
  3, on `feature/roon-transport`. Wrote Phase 3's design decisions and numbered steps into
  IMPL_TRANSPORT.md first (Phase 0/1 were both already done, so — per the plan's own study-first
  precedent — the fine steps could finally be written rather than guessed). Step 3.1 added
  `TransportError::CommandFailed`/`TransportError::NoResponse` and generalized
  `UnexpectedResponse`'s message text off "for a zone subscription" now that `control`/`seek` share
  it with `zones.rs` too. Step 3.2 added `transport::control`: typed `ControlAction`/`SeekHow`
  enums (`PlayPause` needs an explicit `#[serde(rename = "playpause")]` override — plain snake_case
  would produce `play_pause`, which the wire protocol doesn't accept), `control`/`seek` request
  functions, and a `parse_command_response`/`await_command_response` pair handling the single
  `COMPLETE` each verb replies with — `Success` → `Ok(())`, any other name → `CommandFailed`,
  anything but a `COMPLETE` → `UnexpectedResponse`, an empty stream → `NoResponse`. Same
  pure-parser/thin-async-wrapper split `zones.rs`'s `parse_zone_event`/`ZoneSubscription::recv`
  already established, and the same testing precedent: the pure parser and the async wrapper (over
  a fabricated `MooResponseStream::new(rx)`) are unit-tested directly, `control`/`seek` themselves
  aren't beyond type-checking and two enum-serialization assertions — 7 new tests, 79 total. No
  subscription/reconnect handling needed, unlike `zones.rs`: these are one-shot requests. Phase 4
  (volume/output controls) is next.
- Added `core::roon::transport` for zone subscription & state (2026-09-07): IMPL_TRANSPORT.md Phase
  2, on `feature/roon-transport`. Step 2.1 added `transport::model`'s typed
  `Zone`/`Output`/`NowPlaying`/`Volume`/... types (pure `serde::Deserialize`, unit-tested against
  fixture JSON matching docs/protocol/transport.md). Step 2.2 added `"com.roonlabs.transport:2"` to
  `connection`'s `REQUIRED_SERVICES` (previously empty). Step 2.3 added `transport::error`
  (`TransportError`) and `transport::zones` (`subscribe_zones`, `ZoneEvent`, `ZoneSeekChange`,
  `ZoneSubscription`), parsing `CONTINUE Subscribed`/`Changed` messages off a
  `connection::MooResponseStream` into typed events, ending the subscription for good (not
  resyncing) on a malformed or unrecognized response. `connection::requests::MooResponseStream`
  gained a `pub(crate) fn new(rx) -> Self` specifically so this module's tests could fabricate a
  response stream over a plain `mpsc` channel without a live Core, the same pattern
  `connection::requests`'s own tests already use — 13 new tests, 72 total. Flagged, not resolved:
  `transport::TransportError` shares its name with (but is a distinct type from)
  `connection::TransportError` (the MOO websocket transport's error) — code importing both will
  need to alias one on `use`. `core::roon::transport` doesn't yet expose any control verbs
  (`control`/`seek`/`change_volume`/...) or `subscribe_outputs`/`subscribe_queue` — Phase 3
  (playback controls) is next.
- Added `connection::requests` for generic MOO request/response multiplexing (2026-09-07):
  IMPL_TRANSPORT.md Phase 1, on `feature/roon-transport` (branched from `develop`). Step 1.1
  re-exports `MooMessage`/`MooVerb`/`MooBody` from `connection` (were already `pub` within
  `moo::message`, just unreachable from outside `connection` since `moo` stays private per
  CLAUDE.md §1). Step 1.2 adds `ConnectionRequests` (a new `Clone`-able handle, deliberately
  separate from `ConnectionHandle` so any number of future service modules can each hold their own
  clone for the app's whole lifetime while `ConnectionHandle::shutdown` keeps its single-owner,
  consuming shape) plus `MooResponseStream`/`ConnectionRequestError`, wired into
  `run_until_disconnected`'s existing `tokio::select!` loop via a command channel and a per-connection
  request-id/dispatch table (ids start at `3`, since `handshake::register` already hardcodes `1`/`2`
  for its own two steps). `Connection::spawn` now returns `(ConnectionHandle, ConnectionRequests,
  EventReceiver)` — no other call site exists yet to update. Design was confirmed with the user first
  (resolving IMPL_TRANSPORT.md's until-now-deferred Phase 1 open architectural question): reconnect
  handling needs no explicit signal, since the dispatch table and command channel are both local to
  one `run_until_disconnected` call and drop when it returns, closing every open response stream —
  a caller with an active subscription sees it end exactly like a finished one-shot request. Added
  `handle_command`/`dispatch_response` as small standalone functions specifically so this could be
  unit-tested the same way `moo::handshake` already is, purely over `mpsc`/`watch` channels, no live
  Core needed (9 new tests, 58 total). `core::roon::transport` itself still doesn't exist; this only
  built the seam it will use — Phase 2 (zone subscription & state model) is next.
- Completed the `com.roonlabs.transport:2` wire-protocol study (2026-09-07):
  [docs/protocol/transport.md](docs/protocol/transport.md) — IMPL_TRANSPORT.md's Phase 0. Covers the
  `Zone`/`Output`/`Volume`/`NowPlaying`/`QueueItem` data model, `subscribe_zones`/`subscribe_outputs`/
  `subscribe_queue` subscription envelope, all control verbs (`control`, `seek`, `change_volume`,
  `mute`/`mute_all`, `standby`/`toggle_standby`/`convenience_switch`, `change_settings`,
  `transfer_zone`, `play_from_here`, `get_zones`/`get_outputs`), and the `group_outputs`/
  `ungroup_outputs` wire shape (grouping itself stays deferred). Cross-checked against
  `RoonLabs/node-roon-api-transport` (primary/normative, Apache-2.0), `RoonLabs/node-roon-api`'s
  `moo.js` (for subscription-key/reconnect semantics), and the same two community Rust ports
  sood-moo.md used. Key finding for IMPL_TRANSPORT.md's Phase 1 open architectural question: the
  reference implementation's subscription state (`Moo`'s `requests`/`subkey` counters) lives entirely
  in the websocket-connection object and is discarded on disconnect with no Core-side memory either —
  confirms that re-subscribing after a `connection` reconnect is `transport`'s own responsibility, not
  something `connection` needs to (or should) handle on its behalf. Also flags two small
  cross-source divergences worth carrying into implementation: `Output.source_controls` is really an
  array despite the JSDoc's singular formatting (the `control_key` selector on `standby`/
  `toggle_standby`/`convenience_switch` only makes sense if there can be more than one), and
  `change_volume`'s `value` must be a float per the primary source, not the `i32` one Rust port uses.
  No code changes; Phase 1 remains deferred pending a design-confirmation step with the user, per the
  plan.
- Archived the completed core-connection implementation plan (2026-09-07): moved
  `IMPL_CORE_CONNECTION.md` to [docs/IMPL_CORE_CONNECTION.md](docs/IMPL_CORE_CONNECTION.md) now
  that all its phases are done (see Open work above) — kept for reference, not deleted, since it
  records per-step Gate 1 rationale future phases may want to mirror. Never a tracked file, so
  this was a plain filesystem move, not a `git mv`.
- Fixed `moo::handshake::register` to send `required_services`/`optional_services` (2026-09-07):
  the registration body previously only declared `provided_services`, though
  `docs/protocol/sood-moo.md` documents all three as expected — the likely reason a real Core
  closed the connection right after `register` during the Phase 4.1 testing recorded below,
  instead of creating a pending Settings → Extensions entry. `register()` now takes
  `required_services`/`optional_services` parameters alongside the existing `provided_services`
  one; `connection/mod.rs` passes empty slices for both via new `REQUIRED_SERVICES`/
  `OPTIONAL_SERVICES` consts, since no per-service Core API module (`transport:2`, `browse:1`,
  ...) is implemented yet to need one — a future phase adding one fills in the relevant list.
  Follow-up to (not part of) Phase 4; live re-verification against a real Core is left to the
  user rather than run automatically, given what happened during the Phase 4.1 attempt.
- Accepted unit-test-only coverage as final for Phase 4.1 (2026-09-07): per user decision, no
  further automated end-to-end test is planned for `core::roon::connection` — the existing 49
  per-module unit tests plus manual verification against a real Core stand as this module's test
  coverage. Closes out the open decision from the previous entry below. Phase 4 (and
  `IMPL_CORE_CONNECTION.md`'s whole plan) is now complete, pending only the
  `feature/roon-connection-core` → `develop` merge (4.2).
- Attempted Phase 4.1's automated end-to-end integration test, descoped as unsafe (2026-09-07):
  built `core/tests/roon_connection_handshake.rs` — a fake local SOOD responder + mock local
  MOO/WS server (hand-rolled against `docs/protocol/sood-moo.md`'s wire format directly, since
  `sood`/`moo` are private modules per CLAUDE.md §1 and the test can only reach `Connection`'s
  public API) driving a real `Connection` through `Discovering → Connecting → Registering →
  Paired`, `#[ignore]`d by default per an explicit user decision (SOOD discovery has no test seam:
  real multicast, on every local interface, first-reply-wins, so a real Roon Core reachable on the
  network could race the fake one). Running it (with the user's explicit go-ahead, after they
  separately enabled multicast on `lo` — a machine-level `ip link set lo multicast on`, not a repo
  change) confirmed the risk is real, not theoretical: `Connection` connected to something other
  than the fake local server both before and after the `lo` fix, completing a real
  `registry:1/info` round trip before the connection closed — consistent with a real Roon Core on
  the LAN. No pending entry appeared in that Core's Settings → Extensions, which is itself a new
  finding rather than proof of safety: `moo::handshake::register`'s body never sends
  `required_services`/`optional_services` (only `provided_services`), though
  `docs/protocol/sood-moo.md` documents all three as expected — plausibly why a real Core rejects
  the malformed body outright rather than creating a visible pending-pairing entry. Given the real
  Core reliably won the discovery race even with the fake responder reachable over loopback, the
  test was judged not safely re-runnable on a network with a real Core present (this developer's,
  concretely) and was deleted rather than left as a `--ignored` trap for a future run. Discovery-
  to-pairing coverage for now rests on the existing per-module unit tests plus manual verification
  against a real Core — Phase 4.1 is open again pending the user's choice between accepting that as
  final, or scoping a real test seam into `connection` as separate follow-up work.
- Added reconnect-on-disconnect (2026-09-07): `core::roon::connection::run` (the task body behind
  `Connection::spawn`) now loops — on any disconnect other than `ConnectionHandle::shutdown`
  (transport closed, keepalive went stale, a step failed), it reports `Disconnected` and then
  loops back to a fresh `ConnectionState::Discovering` pass instead of returning, per CLAUDE.md's
  mandatory technical choice to never redial a stale address: `discover_first_core` spawns a brand
  new `sood::discovery::run` task each time round the loop, so there's no cached address to redial
  even accidentally. Whether to loop or stop is decided by reading `shutdown_rx`'s current value
  (a `watch::Receiver<bool>`) once `run_until_disconnected` returns, rather than by giving that
  function its own "should I retry" return variant — every one of its exit paths already routes
  through `shutdown_rx` one way or another, so the flag alone is sufficient and simpler than
  threading a second signal through it. This is Phase 3's last step (3.3); Phase 3 (state,
  keepalive, reconnect) is now complete. No new unit tests: the change is a control-flow loop over
  existing, already-tested pipeline steps (discovery/transport/handshake), with no new pure logic
  to isolate — exercising the loop itself needs the fake-SOOD/mock-MOO harness Phase 4.1's
  end-to-end integration test is building next, so real coverage of "does it actually rediscover
  and re-pair" lands there. Reconnect has no backoff yet (flagged as an open gap above, not part of
  this step's scope). Doc comments on `Connection::spawn`, `run_until_disconnected`, and
  `ConnectionState::Disconnected` updated to match — they previously said this step "never retries
  on disconnect."
- Added the connection state machine and public `Connection` API (2026-09-07):
  `core::roon::connection::{state, error, mod}` — `Connection::spawn(ConnectionConfig) ->
  (ConnectionHandle, mpsc::UnboundedReceiver<ConnectionEvent>)` is now the single public entry
  point wiring `sood::discovery` → `moo::transport` → `moo::handshake` → `keepalive` together:
  discover the first Core, connect, run the registry handshake, then loop handling inbound
  `pairing:1`/`ping:1` requests while the keepalive watches for staleness, emitting
  `ConnectionEvent::StateChanged` through `Discovering → Connecting → Registering →
  Paired { core_id }` and `ConnectionEvent::Error`/`Disconnected` on the way out. `sood`/`moo`
  submodules were changed from private to `pub(super)` so `connection/mod.rs` could reach into
  them — they're still unreachable from outside `connection` itself (nothing outside this module
  calls SOOD or reacts to pairing events directly, per CLAUDE.md §1), just no longer unreachable
  from `connection` too. `DiscoveryError`/`TransportError`/`HandshakeError` were promoted from
  `pub(crate)` to `pub` and re-exported from `connection` so `ConnectionError` (now `pub`, wrapping
  all three) doesn't leak an unnameable type across the `dissonanza-core`/`dissonanza` crate
  boundary — caught by `cargo build`'s `private_interfaces` lint, not something CI's `-D warnings`
  clippy pass alone would have surfaced first. `ConnectionConfig` was likewise promoted from
  `pub(crate)` to `pub`, since it's now `Connection::spawn`'s parameter type. Diverges from
  IMPL_CORE_CONNECTION.md's step 3.2 sketch of `error.rs` wrapping `SoodError`/`MooError`
  directly: the actual boundary functions this step calls (`sood::discovery::run`,
  `moo::transport::run`, `moo::handshake::register`/`handle_ping_request`/
  `PairingState::handle_request`) return `DiscoveryError`/`TransportError`/`HandshakeError`
  instead — those are the types that exist at the call sites, so `ConnectionError` wraps those.
  The app-level keepalive timeout (60s, checked every 10s) is a flagged judgment call, not a
  sourced protocol value — `docs/protocol/sood-moo.md` documents the MOO registry handshake and
  the 10s/one-missed-pong WS-level ping but not how often a Core sends `ping:1/ping` requests at
  the application level, which is what this backstop is really watching for. Discovery stops as
  soon as the first candidate Core is found, rather than continuing to run until paired (a
  possible design the `sood::discovery` doc comment previously speculated about) — with no
  reconnect/retry logic yet (that's step 3.3), keeping discovery running past having a usable
  candidate wouldn't currently do anything with any further candidates it found. This step
  intentionally does not reconnect: on any disconnect (transport closed, keepalive stale, a step
  failed, or `ConnectionHandle::shutdown` called), `Connection` reports `Disconnected` and stops
  rather than looping back to a fresh `Discovering` pass — that loop-back is step 3.3, next.
- Added the app-level keepalive staleness health-check (2026-09-07):
  `core::roon::connection::keepalive::Keepalive` tracks when activity (any inbound MOO message)
  was last observed and reports the connection stale once a configurable timeout passes with no
  further activity — the primitive a later connection state machine will use to force
  `Unpaired`/`Disconnected` even if `core_paired`/`core_unpaired` didn't fire, per CLAUDE.md §1
  and the `node-roon-api` `lost_core` bug already documented under `moo::handshake`. Takes `now`
  as an explicit `Instant` parameter on every method instead of reading `Instant::now()`
  internally, so its tests drive the clock deterministically without real sleeps. This is the
  first step of Phase 3 (state, keepalive, reconnect) in the implementation plan. Not wired into
  a connection state machine yet.
- Added the `com.roonlabs.ping:1` responder (2026-09-07): `core::roon::connection::moo::handshake::handle_ping_request`
  replies `COMPLETE Success` to an inbound `ping` request, unknown request names get
  `InvalidRequest` (matching the `com.roonlabs.pairing:1` handler's own fallback). Stateless, so
  no struct like `PairingState` was needed. This was the last piece of Phase 2 (MOO transport &
  handshake) in the implementation plan — connection-establishment protocol work now moves to
  Phase 3 (keepalive, state machine, reconnect). Not wired into a connection state machine yet.
- Added the inbound `com.roonlabs.pairing:1` service handler (2026-09-07):
  `core::roon::connection::moo::handshake::PairingState` responds to `subscribe_pairing`/
  `unsubscribe_pairing`/`get_pairing` with current pairing status and, on an inbound `pair`
  request, marks the connection paired and emits `PairingEvent::Paired { core_id }`. While
  implementing this, cross-checked `node-roon-api`'s `lib.js` (the reference source
  `docs/protocol/sood-moo.md` already cites) and found there is no `unpair` wire message —
  unpairing is inferred purely from the moo websocket closing (and the reference
  implementation has a real missing-braces bug that fires `core_unpaired` for *any* core's
  disconnect, not just the paired one), which is concrete, sourced grounding for why CLAUDE.md
  §1 requires an app-level keepalive backstop rather than trusting `core_paired`/`core_unpaired`
  alone. That disconnect-inferred "unpair" handling is deferred to Phase 3's keepalive/reconnect
  work, not this module. Not wired into a connection state machine yet.
- Added the MOO registry registration handshake (2026-09-07): `core::roon::connection::moo::handshake`
  (`register(...)`) sends `registry:1/info` then `registry:1/register` (declaring caller-supplied
  `provided_services` and an optional saved token), and parses the `COMPLETE Registered` body into
  a typed `Registered { core_id, token, display_name, display_version, provided_services }`.
  Also added `connection::config::ConnectionConfig` (extension identity: `extension_id`,
  `display_name`, `display_version`, `publisher`, `email`, optional `website`). Operates purely
  over `mpsc` channels shaped like `moo::transport`'s outbound/inbound pair, so it's tested without
  a real websocket. Added `serde` (`derive` feature) to `core`'s dependencies to deserialize the
  `Registered` body. Token is in-memory only this phase — persistence is deferred, per the
  implementation plan's non-goals. Not wired into a connection state machine yet.
- Fixed `release.yml` (renamed to `publish.yml`) firing on every push (2026-09-07): root cause was an invalid `if: ${{ secrets.AUR_SSH_PRIVATE_KEY != '' }}` on `publish-aur` — the `secrets` context can't be read in *any* `if:` conditional (job- or step-level), present since the file's first commit. Because the file failed to parse, GitHub couldn't read its tag-only `on:` block and attached a failing "Invalid workflow file" check to every push instead. Fixed per GitHub's documented pattern: surface the secret as a job-level `env:` var, then gate the AUR-publish step's `if:` on `env.AUR_SSH_PRIVATE_KEY` instead. Also renamed `release.yml` → `publish.yml` because the broken file's stale workflow registration (wrong display name, wrong job content shown) survived a same-name delete-and-recreate and needed a fresh filename to clear. Trigger is unchanged: still tag-only (`v*.*.*`), never runs on a plain push.
- Added the MOO websocket transport (2026-09-06): `core::roon::connection::moo::transport`
  (`tokio-tungstenite` + `futures-util` for the split sink/stream) — connects to
  `ws://<addr>/api`, ferries `MooMessage`s to/from binary WS frames over `mpsc` channels, and
  runs an application-level ping/pong keepalive (caller-supplied interval; missed pong closes
  the connection). Framing violations end the loop rather than resyncing. Tested against an
  in-process mock WS server (`tokio_tungstenite::accept_async`). Added `tokio-tungstenite`,
  `futures-util`, and `bytes` to `core`'s dependencies. Not wired into a connection state
  machine yet.
- Added MOO message framing (2026-09-06): `core::roon::connection::moo::message` (`MooMessage`,
  `MooVerb`, `MooBody`, `MooError`) parses/encodes the MOO wire format from
  [docs/protocol/sood-moo.md](docs/protocol/sood-moo.md) — header block, `Request-Id`,
  `Content-Length`/`Content-Type` cross-validation, JSON vs. raw-bytes body. Pure parsing, no I/O;
  not wired into a websocket yet. Added `serde_json` to `core`'s dependencies for JSON body
  handling.
- Added the SOOD multicast discovery loop (2026-09-06): `core::roon::connection::sood::discovery`
  (`if-addrs` for interface enumeration, `socket2` for per-interface multicast socket setup, `tokio`
  for the async loop) — per-interface sockets, 5s re-enumeration, 10s×6-then-60s query cadence, dedupe
  by `unique_id`, `_replyaddr`/`_replyport` override. Not wired into a public `Connection` API yet.
- Scaffolded the Cargo workspace and added SOOD TLV message parsing (2026-09-06): `core/` (crate
  `dissonanza-core`) and `app/` (crate `dissonanza`) crates; `core::roon::connection::sood::message`
  parses/encodes the SOOD wire format from [docs/protocol/sood-moo.md](docs/protocol/sood-moo.md).
  Work is happening on `feature/roon-connection-core`, branched from a newly-created `develop`.
- Project scaffolding filled in: TECH_STACK.md, NORTH-STAR.md, DESIGN.md, CURRENT_STATE.md, CONVENTIONS.md, COPY.md, and CLAUDE.md's open sections, drafted from `docs/roon-linux-remote-client-onderzoek.md` plus follow-up decisions with the user (2026-09-06). CONTEXT.md intentionally left untouched (only ever written on explicit instruction).
- Completed the SOOD/MOO wire-protocol study (2026-09-06): [docs/protocol/sood-moo.md](docs/protocol/sood-moo.md), cross-checked against the official `node-roon-api` SDK and two community Rust ports. Unblocks writing atomic implementation steps for `core::roon::connection` (packet/message formats and the registration/pairing handshake are now documented); per-service message body shapes remain a separate, later study.
- Abandoned custom star ratings (2026-09-06): matching ratings reliably across Roon's multi-version catalog (local rip vs. Tidal vs. remaster, each a distinct Roon item) was judged too complex and not important enough to build. Dissonanza will instead surface Roon's own native track love/unlove control via the Transport API — Roon already syncs this to Last.fm on its own, so no local rating store, track-identity keying, or app-side Last.fm sync is needed. Scrobbling (play-history submission) remains a separate, possible future bonus milestone. Removed CLAUDE.md's "Rating Store" Architectural Contract (Roon Connection renumbered §2 → §1) and the planned `core::rating` module; updated NORTH-STAR.md, TECH_STACK.md, CONVENTIONS.md, COPY.md, DESIGN.md, RELEASES.md, and the Flatpak packaging files accordingly.
- Standardized versioning, changelog, and release-cutting (2026-09-06): Conventional Commits adopted for all commit messages (RELEASES.md's Conventional Commits section, referenced from CLAUDE.md's Git workflow); SemVer with an explicit pre-1.0 bump rule; `git-cliff` (`cliff.toml`) generates `CHANGELOG.md`, `cargo-release` drives the version bump as one deliberate, human-run command on `main` — never automatic on push. `publish.yml`'s GitHub Release body now comes from that generated CHANGELOG.md section instead of GitHub's auto-generated notes. `ci.yml` gained a guardrail job failing PRs into `main` that don't bump the workspace version. Motivated by a versioning mistake observed in another project (`~/git/wiki-md`): automatic per-push version bumps produced a messy, duplicated release/changelog history instead of curated releases.
