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
│               ├── mod.rs           # mod config; mod sood; mod moo; (private — not a public surface yet)
│               ├── config.rs        # ConnectionConfig — extension identity for MOO registration
│               ├── keepalive.rs     # Keepalive — app-level staleness health-check
│               ├── sood/            # private: SOOD discovery, never `pub`
│               │   ├── mod.rs
│               │   ├── message.rs   # SoodMessage/SoodMessageType/SoodError — TLV codec, pure parsing
│               │   └── discovery.rs # per-interface multicast sockets, query cadence, dedupe by unique_id
│               └── moo/             # private: MOO websocket protocol, never `pub`
│                   ├── mod.rs
│                   ├── message.rs   # MooMessage/MooVerb/MooBody/MooError — message framing, pure parsing
│                   ├── transport.rs # websocket connect, MOO frame send/receive, WS ping/pong keepalive
│                   └── handshake.rs # registry:1/info + /register handshake; pairing:1 + ping:1 service responders
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
│   └── protocol/
│       └── sood-moo.md   # SOOD/MOO wire-protocol study — normative reference for core::roon::connection
└── README.md
```

## Modules

- **`core::roon::connection`** (`core/src/roon/connection/`) — per CLAUDE.md §1, the sole owner of
  Core discovery, `core_paired`/`core_unpaired` handling, keepalive, and reconnect. Not a public API
  yet (no `Connection` type exists) — currently just its private internals:
  - `sood::message` — SOOD TLV packet parsing/encoding (`SoodMessage`, `SoodError`). Pure, no I/O.
  - `sood::discovery` — the multicast discovery loop: one send/receive socket per local IPv4
    interface (`socket2`), 5s interface re-enumeration (`if-addrs`), query cadence (10s×6 then 60s),
    dedupe by `unique_id`, `_replyaddr`/`_replyport` override. Exposes `run(...)`, not yet called by
    anything — wiring it into a `Connection` state machine that also owns MOO pairing and knows when
    to stop discovering (Core paired) is separate, not-yet-started work.
  - `moo::message` — MOO message framing (`MooMessage`, `MooVerb`, `MooBody`, `MooError`).
    Parses/encodes the header-block + blank-line + body wire format: `Request-Id` extraction,
    `Content-Length`/`Content-Type` cross-validation, JSON vs. raw-bytes body handling. Pure,
    no I/O.
  - `moo::transport` — the MOO websocket transport (`tokio-tungstenite`): connects to
    `ws://<addr>/api`, encodes/sends outbound `MooMessage`s and decodes/forwards inbound ones as
    binary WS frames over `mpsc` channels, and runs an application-level WS ping every
    (caller-supplied) interval, closing the connection if a pong is missed. A framing violation
    (malformed MOO bytes, or a text frame) ends the loop immediately rather than resyncing.
    Exposes `run(...)`, not yet called by anything.
  - `moo::handshake` — the registry registration handshake: sends `registry:1/info` then
    `registry:1/register` (declaring caller-supplied `provided_services`, plus a saved token if
    the caller has one), and parses the `COMPLETE Registered` body (`core_id`, `token`,
    `display_name`, `display_version`, `provided_services`) into a typed `Registered`. Also
    implements the `com.roonlabs.pairing:1` service this extension provides in return
    (`PairingState`/`PairingEvent`): `subscribe_pairing`/`unsubscribe_pairing`/`get_pairing`
    report current pairing status, and an inbound `pair` request (the Core, when the user pairs
    this extension in Roon's UI) sets it and emits `PairingEvent::Paired`. There is no `unpair`
    wire message — per `node-roon-api`, unpairing is inferred purely from the moo connection
    closing, so it's handled where connection lifecycle is tracked (Phase 3's keepalive/
    reconnect), not here. Also implements the `com.roonlabs.ping:1` service this extension
    provides in return (`handle_ping_request`, stateless): replies `COMPLETE Success` to an
    inbound `ping` request, distinct from the WS-level ping/pong `moo::transport` already runs.
    Operates purely over `mpsc` channels shaped like `moo::transport`'s, so it's tested without a
    real websocket. Exposes `register(...)`, `PairingState::handle_request(...)`, and
    `handle_ping_request(...)`, not yet called by anything.
  - `connection::config` — `ConnectionConfig` (`extension_id`, `display_name`, `display_version`,
    `publisher`, `email`, optional `website`): the extension identity `moo::handshake::register`
    sends during registration.
  - `connection::keepalive` — `Keepalive`: tracks when activity (any inbound MOO message) was
    last observed and reports the connection stale once `timeout` passes with none, regardless of
    whether `core_paired`/`core_unpaired` fired — the primitive a connection state machine will
    use to force `Unpaired`/`Disconnected` on staleness, per CLAUDE.md §1. Takes `now` as an
    explicit `Instant` parameter on every method (rather than reading `Instant::now()`
    internally) so it's tested without real sleeps. Not wired into a connection state machine
    yet.

## Open work

- `core::roon::connection` implementation in progress on `feature/roon-connection-core` (branched from
  a new `develop`, per CLAUDE.md's git workflow): SOOD TLV parsing, the SOOD multicast discovery
  loop, MOO message framing, the MOO websocket transport, the MOO registry registration
  handshake, and the inbound `com.roonlabs.pairing:1`/`com.roonlabs.ping:1` services (handling
  `pair` and `ping` requests) are done (see Modules above) — Phase 2 of the implementation plan is
  complete. The app-level keepalive staleness check (`connection::keepalive::Keepalive`) is also
  now done — Phase 3 has started. Still to build: wiring that keepalive into a connection state
  machine (including the disconnect-inferred "unpair" path — there's no wire message for it, see
  the `moo::handshake` entry above), reconnect-on-disconnect, and the public `Connection` API
  tying it all together — none of these are wired up yet, and `sood::discovery::run`/
  `moo::transport::run`/`moo::handshake::register`/`moo::handshake::PairingState::handle_request`/
  `moo::handshake::handle_ping_request`/`keepalive::Keepalive` aren't called by anything yet.
  - ~~Custom Rust SOOD/MOO protocol implementation needs its own wire-protocol study~~ — **done**, see
    [docs/protocol/sood-moo.md](docs/protocol/sood-moo.md): packet/message formats, the
    connection/registration/pairing handshake, and the keepalive rationale behind CLAUDE.md §1,
    cross-checked against the official `node-roon-api` SDK (Apache-2.0) and two MIT-licensed community
    Rust ports (`shin1ohno/roon-rs`, `TheAppgineer/rust-roon-api` — legally-clear reference/reuse
    material; "not relying on it" was about not taking a dependency, not about the code being
    off-limits). Still open: per-service (transport/browse/image/...) message body shapes aren't
    covered — each needs its own study when that phase starts.
- Slint GUI: not started (app/src/main.rs is a trivial placeholder).
- Pairing-token persistence (so a paired extension doesn't have to re-pair on every restart) is
  deferred until a cache-store phase exists — the MOO handshake step will hold it in memory only.
- Cache invalidation strategy for locally cached album art (when to refresh on a new Roon scan or changed art) undecided.
- DESIGN.md's color tokens, typography scale, and concrete component states are marked TBD pending user-supplied screenshots of Roon's official UI.
- Flatpak submission is blocked on making the repo public (currently private, see TECH_STACK.md) and on generating `cargo-sources.json` once real dependencies are locked in.
- AUR push in `publish.yml` is scaffolded but inert until `AUR_SSH_PRIVATE_KEY`/`AUR_USERNAME`/`AUR_EMAIL` secrets are configured, and until the package is claimed on AUR with a first manual push.
- `PKGBUILD` checksums are placeholders (`SKIP`) until a real `v0.1.0` tag exists.

## Recently changed

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
