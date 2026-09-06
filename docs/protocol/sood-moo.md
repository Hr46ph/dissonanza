# SOOD/MOO wire-protocol study

Status: study complete, unimplemented. Written to unblock `core::roon::connection` (CLAUDE.md
§2) implementation steps — before this existed, "implement SOOD discovery" wasn't an atomic,
well-defined task (see CURRENT_STATE.md's open work). Nothing in this document is copied
verbatim from the sources below; it's an independent description of the wire format, written
from reading and cross-checking them.

**Scope note**: SOOD (discovery) and MOO (the websocket RPC protocol) are the transport
underneath Roon's officially-supported Extension API — the same transport `node-roon-api`
itself uses. Documenting them is not the reverse-engineering NORTH-STAR.md's non-goals rule out
(Roon's *internal* protocol surface for Settings/Audio Setup/Zone config/DSP) — see
CURRENT_STATE.md's open-work entry for that distinction.

## Sources studied

| Source | What it is | License | Role here |
|---|---|---|---|
| [`RoonLabs/node-roon-api`](https://github.com/RoonLabs/node-roon-api) | Official JS SDK, published by Roon Labs | Apache-2.0 | **Primary/normative** — `sood.js`, `moo.js`, `moomsg.js`, `lib.js`, `core.js`, `transport-websocket.js` read in full |
| [`shin1ohno/roon-rs`](https://github.com/shin1ohno/roon-rs) | Community Rust SDK (`roon-api` crate on crates.io) | MIT OR Apache-2.0 | Corroboration + Rust typed-model reference — `crates/roon-sood`, `crates/roon-moo` |
| [`TheAppgineer/rust-roon-api`](https://github.com/TheAppgineer/rust-roon-api) | Community Rust SDK (git-only, alpha) | MIT | Secondary corroboration — `src/sood.rs`, `src/moo.rs` |

All three independently agree on every wire-format detail below (constants, framing, field
encoding) — high confidence. Both Rust ports are permissively licensed and may be read, and
code from them adapted/reused with attribution, per CURRENT_STATE.md's clarification; the
decision to build a custom implementation was about not taking a dependency, not about the
code being off-limits.

## SOOD (Simple Roon Object Discovery)

UDP multicast discovery, used to find a Roon Core's current IP and HTTP/WS port (CLAUDE.md:
"never hardcode a Roon Core's IP/port... the port isn't fixed and can change" — this is why).

**Constants**: multicast group `239.255.90.90`, UDP port `9003` (both send and receive).
Roon Core's discovery service ID: `00720724-5143-4a9b-abac-0e50cba674bb`.

**Packet format** (single UDP datagram):

```
offset  size  field
0       4     magic: ASCII "SOOD"
4       1     version: 0x02
5       1     type: 'Q' (Query) or 'R' (Response)
6..     ...   repeated TLV property entries until end of datagram
```

Each TLV entry:

```
1 byte    name length (must be > 0)
N bytes   name, UTF-8
2 bytes   value length, big-endian
          0xFFFF -> value is null (0 value bytes follow)
          0x0000 -> value is "" (0 value bytes follow)
          else   -> that many bytes follow
M bytes   value, UTF-8 (absent when length is 0xFFFF)
```

Two property names are special and processed by the receiver, not surfaced as data: `_replyaddr`
and `_replyport` — if present, they override the address/port the response is treated as coming
from (used when the socket-level source address isn't the one to actually connect back to).

**Discovering a Core**: send a Query with property `query_service_id` =
`00720724-5143-4a9b-abac-0e50cba674bb`. A Core's Response includes (among possibly other
properties) `service_id` (echoes the query), `unique_id` (stable per-Core identity — dedupe
discovery on this), and `http_port` (the port to open the MOO websocket on — this is the value
that isn't fixed and can change across restarts).

**Socket/interface handling** (from the reference implementation, worth replicating): open one
multicast send+receive socket pair per local IPv4 interface (join the multicast group on each),
plus one shared unbound unicast send socket. Re-enumerate interfaces periodically (reference:
every 5s) and add/drop sockets as interfaces come and go — laptops changing networks is a real
case here, not a hypothetical. All discovery in the reference implementation is IPv4-only; no
IPv6 multicast path exists to study.

**Query cadence**: send a query immediately on start, then periodically while no Core is paired
(reference: every 10s for the first six attempts, then every 60s) — stop once paired. On
disconnect, CLAUDE.md's existing rule already covers this: wait for a new discovery event rather
than retrying the last known address, since the port can change.

## MOO (the connection/RPC protocol)

Runs over a plain (non-TLS) WebSocket, opened at `ws://<core-ip>:<http_port>/api` once SOOD
discovery has produced a candidate address. (If the discovered source IP matches one of the
local machine's own interfaces, the reference implementation substitutes `127.0.0.1` — handles
Core-and-extension running on the same host.)

**Message framing**: each MOO message is a header block (ASCII, `\n`-terminated lines) followed
by a blank line, followed by a raw body if one is declared:

```
MOO/1 <VERB> <name>\n
<Header>: <value>\n
...
\n
<body bytes, Content-Length of them, if Content-Length/Content-Type were present>
```

- `VERB` is one of `REQUEST`, `CONTINUE`, `COMPLETE`.
  - `REQUEST`: `<name>` is `<service>/<method>`, e.g. `com.roonlabs.transport:2/subscribe_zones`.
  - `CONTINUE` / `COMPLETE`: `<name>` is a status string, e.g. `Success`, `Changed`,
    `Subscribed`, `Unsubscribed`, `InvalidRequest`.
- Required header: `Request-Id` (integer, assigned by the REQUEST sender, echoed on every
  CONTINUE/COMPLETE for that exchange).
- Optional headers: `Content-Length` + `Content-Type` (both required together whenever a body is
  present; body is JSON-parsed when `Content-Type` is `application/json`, otherwise treated as
  raw bytes), plus arbitrary other headers (e.g. `Logging: quiet` to suppress a message from
  verbose logging).
- A message with `Content-Type` but no `Content-Length`, or data after the headers with no
  `Content-Length` declared, is malformed — reference implementations close the connection on
  any framing violation rather than trying to resync.

**Request lifecycle**: the requester picks a monotonically increasing `Request-Id` per
connection. The responder may send zero or more `CONTINUE`s sharing that `Request-Id`
(subscriptions/streaming updates use this — a `subscribe_*` call's first `CONTINUE` is the
initial state, later ones are change events), followed by exactly one `COMPLETE`, which closes
that request ID. A `REQUEST` with no matching handler gets `COMPLETE InvalidRequest` back.

**Connection handshake** (the sequence to go from an open websocket to a usable, paired
connection):

1. Open the websocket to `ws://<host>:<port>/api`.
2. Send `REQUEST com.roonlabs.registry:1/info` (no body). The response body carries `core_id` —
   used only to look up a previously-saved pairing token for *this* Core before registering.
3. Send `REQUEST com.roonlabs.registry:1/register` with a body describing this extension:
   `extension_id`, `display_name`, `display_version`, `publisher`, `email`, optionally `website`,
   `required_services`/`optional_services`/`provided_services` (arrays of service-name strings),
   and `token` if step 2 found a saved one for this `core_id`.
4. On success, `COMPLETE Registered` arrives with a body containing at least `core_id`, `token`
   (persist this, keyed by `core_id`, for reconnect without re-pairing), `display_name`,
   `display_version`, and `provided_services` (what the Core actually offers — used to instantiate
   client-side wrappers for the services this extension declared as required/optional).
5. Registering is **not** the same as being paired for control. To receive Roon's actual
   pair/unpair signal, the extension must itself provide the `com.roonlabs.pairing:1` service
   (subscribe/get/pair methods) as one of its `provided_services` — the Core calls `pair` on it
   when the user pairs this extension in Roon's UI. **This is the direct source of CLAUDE.md
   §2's rule that `core_paired`/`core_unpaired` needs an app-level keepalive/health-check on
   top** — the reference implementation's own comments note this event pair is known not to
   always fire correctly, which is exactly why that architectural contract exists.
6. Provide `com.roonlabs.ping:1` (a required provided service in the reference implementation)
   so the Core can verify liveness at the MOO-message level.

**Keepalive, separate from MOO messages**: the websocket transport itself does standard WS
ping/pong (reference: ping every 10s, connection considered dead and closed after one missed
pong). This is transport-level liveness, distinct from — and in addition to — the
`core_paired`/`core_unpaired` health-check contract in CLAUDE.md §2.

## Rust design notes (for whoever implements `core::roon`)

Not part of the wire protocol itself, but worth recording since both Rust ports independently
converged on the same shape, which is a reasonable signal for our own design:

- Split mirrors CLAUDE.md §2's ownership boundary well: `roon-rs` structures this as separate
  `roon-sood` / `roon-moo` crates: SOOD parsing (`SoodMessage { from, msg_type: Query|Response,
  props: HashMap<String, Option<String>> }`) is fully decoupled from MOO framing (`MooMessage {
  verb: Request|Continue|Complete, name, request_id: u32, headers, body: Option<Json|Binary> }`).
  `core::roon::connection` can internally follow the same split without violating the
  single-owner contract — it's one module's internal structure, not multiple owners.
  - `Option<String>` for SOOD prop values (not just `String`) exists specifically to represent
    the `0xFFFF` null-value sentinel, distinct from an empty string.
- Both Rust ports use `thiserror` typed error enums for parse failures (`SoodError`,
  a MOO equivalent) — matches CONVENTIONS.md's error-handling rule already, not a new
  convention.

## What this study deliberately leaves open

- **Full property enumeration**: only the SOOD properties actually consumed by the reference
  implementation (`query_service_id`, `service_id`, `unique_id`, `http_port`, `_replyaddr`,
  `_replyport`) are documented here. A real Response may carry more; the TLV format is
  self-describing, so an implementation can parse and retain unknown properties without needing
  this document extended first.
- **Service-specific message bodies** (transport, browse, image, etc.) — this study only covers
  the connection-establishment layer (registry + pairing + ping) common to every extension, not
  the payload shape of individual API modules. Each service (`com.roonlabs.transport:2`,
  `com.roonlabs.browse:1`, ...) will need its own request/response body study when that phase
  starts.
- **IPv6**: no IPv6 discovery path was found in any of the three sources studied; assume
  IPv4-only for SOOD until/unless contradicted.
