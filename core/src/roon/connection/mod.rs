//! The sole owner of Core discovery, connection, registration, and pairing, per CLAUDE.md §1.
//! [`Connection::spawn`] is the single public entry point — nothing outside this module calls
//! `sood`/`moo` directly.

mod config;
mod error;
mod keepalive;
mod moo;
mod requests;
mod sood;
mod state;

pub use config::ConnectionConfig;
pub use error::ConnectionError;
pub use moo::handshake::HandshakeError;
pub use moo::message::{MooBody, MooMessage, MooVerb};
pub use moo::transport::TransportError;
pub use requests::{ConnectionRequestError, ConnectionRequests, MooResponseStream};
pub use sood::discovery::DiscoveryError;
pub use state::{ConnectionEvent, ConnectionState};

use std::collections::HashMap;
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use keepalive::Keepalive;
use moo::handshake::{self, PairingEvent, PairingState};
use moo::transport;
use requests::{CommandTx, ConnectionCommand};
use sood::discovery::{self, DiscoveredCore};

/// Services this extension provides and must handle inbound requests for, declared during
/// `moo::handshake::register` and dispatched on by [`provided_service_for`] below.
const PROVIDED_SERVICES: &[&str] = &[handshake::PAIRING_SERVICE, handshake::PING_SERVICE];

/// Core-provided services this extension needs (`required_services`) or optionally uses
/// (`optional_services`), declared during `moo::handshake::register` per
/// `docs/protocol/sood-moo.md`. `com.roonlabs.transport:2` is required now that
/// `core::roon::transport` exists (IMPL_TRANSPORT.md Phase 2), and `com.roonlabs.browse:1` now
/// that `core::roon::browse` exists (IMPL_BROWSE.md Phase 1) — both kept as literals here rather
/// than constants imported from their own modules, so `connection` stays ignorant of them
/// specifically beyond needing their names to register, per CLAUDE.md §1. `image:1` will add its
/// own entry when implemented.
const REQUIRED_SERVICES: &[&str] = &["com.roonlabs.transport:2", "com.roonlabs.browse:1"];
const OPTIONAL_SERVICES: &[&str] = &[];

/// How long the app-level keepalive tolerates no inbound MOO activity (a `pair` request, a
/// `ping:1/ping` request, anything at all) before treating the connection as stale and ending
/// it, per CLAUDE.md §1. `docs/protocol/sood-moo.md` doesn't document a Core-side ping cadence
/// to derive this from, so it's a deliberately generous judgment call, well above the transport's
/// own 10s/one-missed-pong WS-level keepalive — this backstop exists for the case that layer
/// misses: the socket itself stays open, but `core_paired`/`core_unpaired` at the application
/// level doesn't.
const KEEPALIVE_TIMEOUT: Duration = Duration::from_secs(60);
/// How often to check the keepalive for staleness.
const KEEPALIVE_CHECK_INTERVAL: Duration = Duration::from_secs(10);

/// Which service this extension provides an inbound `REQUEST` is addressed to, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProvidedService {
    Pairing,
    Ping,
}

/// Classifies a `REQUEST`'s name by which provided service it belongs to, matching on the exact
/// `<service>/` prefix so e.g. a hypothetical `com.roonlabs.pairing:10/...` service can't be
/// mistaken for `com.roonlabs.pairing:1/...`.
fn provided_service_for(request_name: &str) -> Option<ProvidedService> {
    if request_name
        .strip_prefix(handshake::PAIRING_SERVICE)
        .is_some_and(|rest| rest.starts_with('/'))
    {
        return Some(ProvidedService::Pairing);
    }
    if request_name
        .strip_prefix(handshake::PING_SERVICE)
        .is_some_and(|rest| rest.starts_with('/'))
    {
        return Some(ProvidedService::Ping);
    }
    None
}

/// A handle to a spawned [`Connection`]. Dropping it leaves the connection running in the
/// background; call [`ConnectionHandle::shutdown`] to stop it and wait for it to finish.
#[derive(Debug)]
pub struct ConnectionHandle {
    shutdown_tx: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl ConnectionHandle {
    /// Signals the connection to stop and waits for it to finish.
    pub async fn shutdown(self) {
        let _ = self.shutdown_tx.send(true);
        let _ = self.task.await;
    }
}

/// The public entry point for `core::roon::connection`, per CLAUDE.md §1.
pub struct Connection;

impl Connection {
    /// Spawns the connection pipeline in the background: SOOD discovery for a Roon Core, the MOO
    /// registry handshake, then handling inbound `com.roonlabs.pairing:1`/`com.roonlabs.ping:1`
    /// requests with an app-level keepalive, until shutdown is requested. Returns a handle to
    /// stop it, a [`ConnectionRequests`] other `core::roon` modules can clone and use to send
    /// their own MOO requests over whichever connection is currently up (IMPL_TRANSPORT.md Phase
    /// 1), and a channel of [`ConnectionEvent`]s reporting its progress. Any other disconnect
    /// (transport closed, keepalive went stale, a step failed) loops back to a fresh
    /// `Discovering` pass instead of stopping — SOOD discovery starts over from scratch each
    /// time, so a Core's address is never redialed, per CLAUDE.md's mandatory technical choices.
    pub fn spawn(
        config: ConnectionConfig,
    ) -> (
        ConnectionHandle,
        ConnectionRequests,
        mpsc::UnboundedReceiver<ConnectionEvent>,
    ) {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (requests_tx, requests_rx) = watch::channel::<Option<CommandTx>>(None);
        let task = tokio::spawn(run(config, event_tx, shutdown_rx, requests_tx));
        (
            ConnectionHandle { shutdown_tx, task },
            ConnectionRequests::new(requests_rx),
            event_rx,
        )
    }
}

fn send_state(event_tx: &mpsc::UnboundedSender<ConnectionEvent>, state: ConnectionState) {
    let _ = event_tx.send(ConnectionEvent::StateChanged(state));
}

async fn run(
    config: ConnectionConfig,
    event_tx: mpsc::UnboundedSender<ConnectionEvent>,
    mut shutdown_rx: watch::Receiver<bool>,
    requests_tx: watch::Sender<Option<CommandTx>>,
) {
    loop {
        send_state(&event_tx, ConnectionState::Discovering);

        if let Err(err) =
            run_until_disconnected(&config, &event_tx, &mut shutdown_rx, &requests_tx).await
        {
            let _ = event_tx.send(ConnectionEvent::Error(err));
        }

        // Whatever `run_until_disconnected` published while it ran is now stale — nothing should
        // be able to send a request against a connection that just ended, whether this loop is
        // about to retry `Discovering` or stop for good.
        requests_tx.send_replace(None);
        send_state(&event_tx, ConnectionState::Disconnected);

        // `shutdown_rx`'s current value distinguishes a shutdown-requested end (stop) from every
        // other one (loop back to a fresh `Discovering` pass) — `run_until_disconnected` returns
        // `Ok(())` for both, since every exit path already routes through `shutdown_rx` one way
        // or another, so the flag itself is the single source of truth here.
        if *shutdown_rx.borrow() {
            return;
        }
    }
}

/// Runs discovery → connect → register, then the pairing/ping request-response loop, until the
/// connection ends. `Ok(())` covers every clean end (shutdown requested, keepalive went stale,
/// the transport closed on its own with no error); `Err` means a step failed and should be
/// surfaced before `run` reports `Disconnected`. Either way, `run` (the caller) decides whether
/// to loop back to `Discovering` or stop, based on `shutdown_rx`'s value once this returns.
async fn run_until_disconnected(
    config: &ConnectionConfig,
    event_tx: &mpsc::UnboundedSender<ConnectionEvent>,
    shutdown_rx: &mut watch::Receiver<bool>,
    requests_tx: &watch::Sender<Option<CommandTx>>,
) -> Result<(), ConnectionError> {
    let Some(core) = discover_first_core(shutdown_rx).await? else {
        return Ok(()); // shutdown requested before any Core was found
    };

    send_state(event_tx, ConnectionState::Connecting);

    let (outbound_tx, outbound_rx) = mpsc::unbounded_channel::<MooMessage>();
    let (inbound_tx, mut inbound_rx) = mpsc::unbounded_channel::<MooMessage>();
    let (transport_stop_tx, transport_stop_rx) = watch::channel(false);
    let transport_task = tokio::spawn(transport::run(
        core.moo_addr(),
        transport::DEFAULT_PING_INTERVAL,
        outbound_rx,
        inbound_tx,
        transport_stop_rx,
    ));

    send_state(event_tx, ConnectionState::Registering);

    // A shutdown request racing the handshake is handled the same way as every other exit path
    // below: stop the transport and return cleanly. No surrounding loop is needed here (unlike
    // the request loop further down) — `handshake::register` isn't re-entrant across polls, and
    // nothing flips `shutdown_rx` back to `false` in between, so a single race is enough.
    let registered = tokio::select! {
        _ = shutdown_rx.changed() => {
            return stop_transport_and_finish(transport_stop_tx, transport_task).await;
        }
        result = handshake::register(
            &outbound_tx,
            &mut inbound_rx,
            config,
            REQUIRED_SERVICES,
            OPTIONAL_SERVICES,
            PROVIDED_SERVICES,
            None,
        ) => {
            result?
        }
    };

    let mut pairing = PairingState::default();
    // Gates the staleness check below: before the Core actually pairs, the human may take a
    // while to approve it in Roon's own UI, and the Core sends nothing at the MOO level while
    // waiting — normal, expected silence, not staleness. Only once paired does silence past
    // `KEEPALIVE_TIMEOUT` mean the connection genuinely went quiet.
    let mut paired = false;
    let mut keepalive = Keepalive::new(KEEPALIVE_TIMEOUT, Instant::now());
    let mut keepalive_check = tokio::time::interval(KEEPALIVE_CHECK_INTERVAL);

    // Request-ids 1 and 2 are `handshake::register`'s own (already spent, above) — this loop's
    // allocator starts right after them. Scoped to this connection attempt only, like everything
    // else below, matching sood-moo.md: the requester picks a monotonically increasing
    // Request-Id per connection, not across reconnects.
    let mut next_request_id: u32 = 3;
    let mut pending: HashMap<u32, mpsc::UnboundedSender<MooMessage>> = HashMap::new();
    let (command_tx, mut command_rx) = mpsc::unbounded_channel::<ConnectionCommand>();
    // Other `core::roon` modules can now send requests over this connection via
    // `ConnectionRequests` (IMPL_TRANSPORT.md Phase 1) — available as soon as the handshake is
    // done, not gated on `Paired`; see `requests.rs`'s doc comment for why.
    requests_tx.send_replace(Some(command_tx));

    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    return stop_transport_and_finish(transport_stop_tx, transport_task).await;
                }
            }
            _ = keepalive_check.tick() => {
                if paired && keepalive.is_stale(Instant::now()) {
                    return stop_transport_and_finish(transport_stop_tx, transport_task).await;
                }
            }
            // `command_rx` can only observe `None` (all senders dropped) after this function has
            // already returned and dropped its own `command_tx` clone — the clone published into
            // `requests_tx` above keeps at least one sender alive for this whole loop, so the
            // `None` arm here is unreachable in practice; handled as a no-op rather than ending
            // the connection, since losing every `ConnectionRequests` clone says nothing about
            // whether the MOO connection itself is still fine.
            maybe_cmd = command_rx.recv() => {
                if let Some(cmd) = maybe_cmd {
                    handle_command(cmd, &outbound_tx, &mut pending, &mut next_request_id);
                }
            }
            maybe_msg = inbound_rx.recv() => {
                let Some(msg) = maybe_msg else {
                    // The transport ended on its own (peer closed, WS ping/pong failed, a
                    // framing error) rather than us telling it to — join it to see whether that
                    // was an error worth surfacing.
                    return match transport_task.await {
                        Ok(result) => result.map_err(ConnectionError::from),
                        Err(_join_err) => Ok(()),
                    };
                };

                keepalive.record_activity(Instant::now());
                if msg.verb == MooVerb::Request {
                    match provided_service_for(&msg.name) {
                        Some(ProvidedService::Pairing) => {
                            let event = pairing.handle_request(&outbound_tx, &registered.core_id, &msg)?;
                            if let Some(PairingEvent::Paired { core_id }) = event {
                                paired = true;
                                keepalive.record_activity(Instant::now());
                                send_state(event_tx, ConnectionState::Paired { core_id });
                            }
                        }
                        Some(ProvidedService::Ping) => {
                            handshake::handle_ping_request(&outbound_tx, &msg)?;
                        }
                        None => {}
                    }
                } else {
                    // A CONTINUE/COMPLETE responding to a request some `ConnectionRequests`
                    // caller sent — forward it if still awaited. An unmatched request-id here
                    // means the caller already stopped listening or this is stray traffic —
                    // harmless either way, dropped silently like today.
                    dispatch_response(msg, &mut pending);
                }
            }
        }
    }
}

/// Handles one command from a `ConnectionRequests` caller: allocates the next request-id, sends
/// the outbound `REQUEST`, and registers `response_tx` to receive its `CONTINUE`/`COMPLETE`s —
/// but only if the send succeeded, so a dead transport doesn't accumulate registrations nothing
/// will ever remove.
fn handle_command(
    cmd: ConnectionCommand,
    outbound_tx: &mpsc::UnboundedSender<MooMessage>,
    pending: &mut HashMap<u32, mpsc::UnboundedSender<MooMessage>>,
    next_request_id: &mut u32,
) {
    let ConnectionCommand::SendRequest {
        name,
        body,
        response_tx,
    } = cmd;
    let request_id = *next_request_id;
    *next_request_id += 1;
    let sent = outbound_tx.send(MooMessage {
        verb: MooVerb::Request,
        name,
        request_id,
        headers: HashMap::new(),
        body: body.map(MooBody::Json),
    });
    if sent.is_ok() {
        pending.insert(request_id, response_tx);
    }
}

/// Routes one inbound `CONTINUE`/`COMPLETE` to whichever `ConnectionRequests` caller is awaiting
/// it (by request-id), dropping the registration once it's a `COMPLETE` or the caller's stream is
/// gone — lazy cleanup: an abandoned [`requests::MooResponseStream`] is only reaped the next time
/// a message for it fails to forward, not proactively on drop. Returns whether anything matched,
/// for tests; the caller in `run_until_disconnected` doesn't need the value, an unmatched id is
/// silently dropped either way.
fn dispatch_response(
    msg: MooMessage,
    pending: &mut HashMap<u32, mpsc::UnboundedSender<MooMessage>>,
) -> bool {
    let request_id = msg.request_id;
    let is_complete = msg.verb == MooVerb::Complete;
    let forwarded = pending
        .get(&request_id)
        .is_some_and(|response_tx| response_tx.send(msg).is_ok());
    if !forwarded || is_complete {
        pending.remove(&request_id);
    }
    forwarded
}

/// Signals the transport to stop and waits for it, reporting a clean end regardless of what the
/// transport itself returns — used by every "we decided to end the connection ourselves" path
/// (shutdown, keepalive staleness), as opposed to the transport ending unprompted.
async fn stop_transport_and_finish(
    transport_stop_tx: watch::Sender<bool>,
    transport_task: JoinHandle<Result<(), transport::TransportError>>,
) -> Result<(), ConnectionError> {
    let _ = transport_stop_tx.send(true);
    let _ = transport_task.await;
    Ok(())
}

/// Waits for the first Core SOOD discovery finds and stops discovery once one arrives, or
/// returns `None` if `shutdown_rx` fired first. A discovery error is only surfaced if no
/// candidate was found before discovery ended — once we have a candidate we've moved on to
/// using it, and no longer care why discovery itself later exits.
async fn discover_first_core(
    shutdown_rx: &mut watch::Receiver<bool>,
) -> Result<Option<DiscoveredCore>, ConnectionError> {
    let (discovered_tx, mut discovered_rx) = mpsc::unbounded_channel();
    let (stop_tx, stop_rx) = watch::channel(false);
    let task = tokio::spawn(discovery::run(discovered_tx, stop_rx));

    let found = loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break None;
                }
            }
            maybe_core = discovered_rx.recv() => break maybe_core,
        }
    };

    let _ = stop_tx.send(true);
    match (found, task.await) {
        (Some(core), _) => Ok(Some(core)),
        (None, Ok(Ok(()))) => Ok(None),
        (None, Ok(Err(err))) => Err(err.into()),
        (None, Err(_join_err)) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_pairing_and_ping_requests() {
        assert_eq!(
            provided_service_for("com.roonlabs.pairing:1/subscribe_pairing"),
            Some(ProvidedService::Pairing)
        );
        assert_eq!(
            provided_service_for("com.roonlabs.ping:1/ping"),
            Some(ProvidedService::Ping)
        );
    }

    #[test]
    fn rejects_unrelated_and_similarly_prefixed_names() {
        assert_eq!(
            provided_service_for("com.roonlabs.transport:2/subscribe_zones"),
            None
        );
        // Must not treat a hypothetical `pairing:10` service as a `pairing:1` request just
        // because the string happens to start with the same characters.
        assert_eq!(
            provided_service_for("com.roonlabs.pairing:10/subscribe_pairing"),
            None
        );
    }

    fn continue_msg(request_id: u32) -> MooMessage {
        MooMessage {
            verb: MooVerb::Continue,
            name: "Changed".to_string(),
            request_id,
            headers: HashMap::new(),
            body: None,
        }
    }

    fn complete_msg(request_id: u32) -> MooMessage {
        MooMessage {
            verb: MooVerb::Complete,
            name: "Success".to_string(),
            request_id,
            headers: HashMap::new(),
            body: None,
        }
    }

    #[test]
    fn handle_command_sends_request_starting_at_id_three_and_registers_pending() {
        let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<MooMessage>();
        let (response_tx, mut response_rx) = mpsc::unbounded_channel::<MooMessage>();
        let mut pending = HashMap::new();
        let mut next_request_id = 3;

        handle_command(
            ConnectionCommand::SendRequest {
                name: "com.roonlabs.transport:2/subscribe_zones".to_string(),
                body: Some(serde_json::json!({"subscription_key": 0})),
                response_tx,
            },
            &outbound_tx,
            &mut pending,
            &mut next_request_id,
        );

        assert_eq!(next_request_id, 4, "allocator advances past the id it used");

        let sent = outbound_rx.try_recv().expect("a REQUEST was sent");
        assert_eq!(sent.verb, MooVerb::Request);
        assert_eq!(sent.name, "com.roonlabs.transport:2/subscribe_zones");
        assert_eq!(
            sent.request_id, 3,
            "starts after handshake's own ids 1 and 2"
        );
        assert_eq!(
            sent.body,
            Some(MooBody::Json(serde_json::json!({"subscription_key": 0})))
        );

        // Registered under the id it was just given: a response for that id reaches it.
        assert!(dispatch_response(continue_msg(3), &mut pending));
        response_rx.try_recv().expect("forwarded to the caller");
    }

    #[test]
    fn handle_command_skips_registration_when_transport_is_gone() {
        let (outbound_tx, outbound_rx) = mpsc::unbounded_channel::<MooMessage>();
        drop(outbound_rx); // simulates the transport having already ended
        let (response_tx, _response_rx) = mpsc::unbounded_channel::<MooMessage>();
        let mut pending = HashMap::new();
        let mut next_request_id = 3;

        handle_command(
            ConnectionCommand::SendRequest {
                name: "com.roonlabs.transport:2/get_zones".to_string(),
                body: None,
                response_tx,
            },
            &outbound_tx,
            &mut pending,
            &mut next_request_id,
        );

        assert!(
            pending.is_empty(),
            "nothing to route responses to if the request was never actually sent"
        );
    }

    #[test]
    fn dispatch_response_keeps_pending_open_across_continues_and_closes_on_complete() {
        let (response_tx, mut response_rx) = mpsc::unbounded_channel::<MooMessage>();
        let mut pending = HashMap::new();
        pending.insert(5, response_tx);

        assert!(dispatch_response(continue_msg(5), &mut pending));
        assert!(
            pending.contains_key(&5),
            "a subscription stays open across CONTINUEs"
        );
        response_rx.try_recv().expect("first CONTINUE forwarded");

        assert!(dispatch_response(complete_msg(5), &mut pending));
        assert!(!pending.contains_key(&5), "COMPLETE closes the request");
        response_rx.try_recv().expect("COMPLETE forwarded too");
    }

    #[test]
    fn dispatch_response_ignores_unmatched_request_id() {
        let mut pending = HashMap::new();
        assert!(!dispatch_response(continue_msg(99), &mut pending));
    }

    #[test]
    fn dispatch_response_reaps_pending_entry_once_caller_stream_is_dropped() {
        let (response_tx, response_rx) = mpsc::unbounded_channel::<MooMessage>();
        drop(response_rx); // the caller lost interest without unsubscribing
        let mut pending = HashMap::new();
        pending.insert(7, response_tx);

        assert!(!dispatch_response(continue_msg(7), &mut pending));
        assert!(
            !pending.contains_key(&7),
            "a failed forward reaps the entry lazily, per requests.rs's doc comment"
        );
    }
}
