//! MOO websocket transport: opens the connection to a Roon Core's MOO endpoint and keeps it
//! alive with an application-level WS ping/pong, per `docs/protocol/sood-moo.md`.
//!
//! Driven by `connection/mod.rs`, which layers `moo::handshake`'s registration/pairing/ping
//! request-response traffic on top of this transport's outbound/inbound channels.

use std::net::SocketAddr;
use std::time::Duration;

use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Error as WsError;
use tokio_tungstenite::tungstenite::Message;

use super::message::{MooError, MooMessage};

/// The ping cadence real callers should use — every 10s, per `docs/protocol/sood-moo.md`.
/// `run` takes the interval as a parameter (rather than hardcoding this) so tests can use a
/// much shorter one instead of waiting on a real 10s cadence.
pub(crate) const DEFAULT_PING_INTERVAL: Duration = Duration::from_secs(10);

/// Bounds the initial WS connect step (`connect_async`) for a SOOD-discovered address — a safety
/// net that should practically never trigger, since a fresh SOOD response already confirms the
/// Core is alive on the LAN right now. Not sourced from `docs/protocol/sood-moo.md` (which doesn't
/// discuss connect-level timeouts at all) — a deliberately generous judgment call, mirroring
/// `connection`'s own `KEEPALIVE_TIMEOUT` precedent. Without this, a stale or unreachable address
/// could otherwise hang on `connect_async` for however long the OS takes to give up on the TCP
/// handshake, rather than failing fast back into `connection::run`'s existing retry loop.
pub(crate) const DISCOVERY_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Bounds [`probe`]'s cached-address fast-path attempt, raced against SOOD discovery on process
/// startup (`connection::mod::resolve_moo_addr`). Deliberately shorter than
/// `DISCOVERY_CONNECT_TIMEOUT`: a stale cached address should lose that race to discovery
/// promptly rather than hold it up — discovery is already running concurrently and unaffected
/// either way.
pub(crate) const FAST_PATH_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("failed to connect to {addr}: {source}")]
    Connect {
        addr: SocketAddr,
        #[source]
        source: WsError,
    },
    #[error("timed out after {timeout:?} connecting to {addr}")]
    ConnectTimeout { addr: SocketAddr, timeout: Duration },
    #[error("failed to send a websocket frame: {0}")]
    Send(#[source] WsError),
    #[error("failed to read a websocket frame: {0}")]
    Receive(#[source] WsError),
    #[error("received a text frame; MOO only ever uses binary frames")]
    UnexpectedTextFrame,
    #[error("received a malformed MOO message: {0}")]
    Framing(#[from] MooError),
    #[error("no pong received within {0:?} of the last ping; connection considered dead")]
    PongTimeout(Duration),
}

/// Connects to a Roon Core's MOO endpoint (`ws://<addr>/api`) and runs the transport loop
/// until `stop_rx` reports `true`, the peer closes the connection, or a transport-level error
/// occurs.
///
/// Every [`MooMessage`] sent on `outbound_rx` is encoded and written as one binary WS frame;
/// every binary WS frame received is decoded and forwarded on `inbound_tx`. Alongside that, a
/// WS ping is sent every `ping_interval`; if no pong has arrived by the time the next ping is
/// due, the connection is considered dead. Framing violations (malformed MOO bytes, or a text
/// frame — MOO never uses those) end the loop immediately rather than trying to resync, per
/// `docs/protocol/sood-moo.md`. The initial connect itself is bounded by `connect_timeout`
/// (callers should pass [`DISCOVERY_CONNECT_TIMEOUT`] or a fast-path-specific bound) — everything
/// after a successful connect is unbounded here, governed by `ping_interval`'s own liveness check
/// instead.
pub(crate) async fn run(
    addr: SocketAddr,
    connect_timeout: Duration,
    ping_interval: Duration,
    mut outbound_rx: mpsc::UnboundedReceiver<MooMessage>,
    inbound_tx: mpsc::UnboundedSender<MooMessage>,
    mut stop_rx: watch::Receiver<bool>,
) -> Result<(), TransportError> {
    if *stop_rx.borrow() {
        return Ok(());
    }

    let (ws_stream, _response) =
        tokio::time::timeout(connect_timeout, connect_async(format!("ws://{addr}/api")))
            .await
            .map_err(|_elapsed| TransportError::ConnectTimeout {
                addr,
                timeout: connect_timeout,
            })?
            .map_err(|source| TransportError::Connect { addr, source })?;
    let (mut sink, mut stream) = ws_stream.split();

    let mut ping_interval = tokio::time::interval(ping_interval);
    // `interval` fires its first tick immediately; consume it here so the first real
    // liveness probe happens one full interval after connecting, not the instant the
    // handshake finishes.
    ping_interval.tick().await;
    let mut awaiting_pong = false;

    loop {
        tokio::select! {
            changed = stop_rx.changed() => {
                if changed.is_err() || *stop_rx.borrow() {
                    let _ = sink.close().await;
                    return Ok(());
                }
            }
            _ = ping_interval.tick() => {
                if awaiting_pong {
                    return Err(TransportError::PongTimeout(ping_interval.period()));
                }
                sink.send(Message::Ping(Bytes::new()))
                    .await
                    .map_err(TransportError::Send)?;
                awaiting_pong = true;
            }
            outbound = outbound_rx.recv() => {
                match outbound {
                    Some(msg) => {
                        sink.send(Message::Binary(Bytes::from(msg.encode())))
                            .await
                            .map_err(TransportError::Send)?;
                    }
                    // The caller dropped its sender — nothing left for us to send, and the
                    // caller is done with this transport.
                    None => return Ok(()),
                }
            }
            incoming = stream.next() => {
                match incoming {
                    Some(Ok(Message::Pong(_))) => {
                        awaiting_pong = false;
                    }
                    // tungstenite answers incoming pings automatically at the protocol
                    // level; nothing for us to do here.
                    Some(Ok(Message::Ping(_))) => {}
                    Some(Ok(Message::Binary(bytes))) => {
                        // A MOO message always has a non-empty header block, so a zero-length
                        // frame can never be a real one — observed as a connection-reset
                        // artifact, per docs/protocol/sood-moo.md's Empty binary WS frames
                        // section; safe to ignore as a no-op rather than a fatal framing error.
                        if bytes.is_empty() {
                            continue;
                        }
                        let msg = MooMessage::decode(&bytes)?;
                        if inbound_tx.send(msg).is_err() {
                            // The caller dropped its receiver — nothing left to deliver to.
                            return Ok(());
                        }
                    }
                    Some(Ok(Message::Text(_))) => return Err(TransportError::UnexpectedTextFrame),
                    Some(Ok(Message::Close(_))) | None => return Ok(()),
                    Some(Ok(Message::Frame(_))) => {
                        unreachable!("tungstenite never yields Message::Frame from a read")
                    }
                    Some(Err(source)) => return Err(TransportError::Receive(source)),
                }
            }
        }
    }
}

/// Probes whether a MOO websocket can be opened at `addr` within `timeout`, closing it
/// immediately either way — this is only a liveness/reachability check for
/// `connection::mod::resolve_moo_addr`'s race against SOOD discovery, never a connection used for
/// anything else (no MOO `REQUEST` is ever sent over it, so it's inert from the Core's
/// registry/pairing bookkeeping). Keeps the raw `tokio_tungstenite` call localized to this module,
/// matching `run`'s own role owning all websocket concerns, rather than having `connection::mod`
/// import it directly.
pub(crate) async fn probe(addr: SocketAddr, timeout: Duration) -> bool {
    let Ok(Ok((mut ws_stream, _response))) =
        tokio::time::timeout(timeout, connect_async(format!("ws://{addr}/api"))).await
    else {
        return false;
    };
    let _ = ws_stream.close(None).await;
    true
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;

    use super::super::message::MooVerb;
    use super::*;

    async fn bind_mock_server() -> (SocketAddr, TcpListener) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let addr = listener.local_addr().expect("local addr");
        (addr, listener)
    }

    #[tokio::test]
    async fn send_receive_round_trip() {
        let (addr, listener) = bind_mock_server().await;

        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.expect("accept");
            let ws = accept_async(tcp).await.expect("server handshake");
            let (mut server_sink, mut server_stream) = ws.split();

            let received = server_stream
                .next()
                .await
                .expect("a frame")
                .expect("a valid frame");
            let bytes = match received {
                Message::Binary(bytes) => bytes,
                other => panic!("expected a binary frame, got {other:?}"),
            };
            let request = MooMessage::decode(&bytes).expect("valid moo message");

            let reply = MooMessage {
                verb: MooVerb::Complete,
                name: "Success".to_string(),
                request_id: request.request_id,
                headers: HashMap::new(),
                body: None,
            };
            server_sink
                .send(Message::Binary(Bytes::from(reply.encode())))
                .await
                .expect("send reply");
        });

        let (outbound_tx, outbound_rx) = mpsc::unbounded_channel();
        let (inbound_tx, mut inbound_rx) = mpsc::unbounded_channel();
        let (_stop_tx, stop_rx) = watch::channel(false);

        let client = tokio::spawn(run(
            addr,
            Duration::from_secs(5),
            Duration::from_secs(30),
            outbound_rx,
            inbound_tx,
            stop_rx,
        ));

        outbound_tx
            .send(MooMessage {
                verb: MooVerb::Request,
                name: "com.roonlabs.registry:1/info".to_string(),
                request_id: 1,
                headers: HashMap::new(),
                body: None,
            })
            .expect("queue outbound message");

        let reply = tokio::time::timeout(Duration::from_secs(5), inbound_rx.recv())
            .await
            .expect("reply arrives before timeout")
            .expect("reply channel open");
        assert_eq!(reply.name, "Success");
        assert_eq!(reply.request_id, 1);

        server.await.expect("server task");
        drop(outbound_tx);
        client.abort();
    }

    #[tokio::test]
    async fn malformed_frame_closes_connection() {
        let (addr, listener) = bind_mock_server().await;

        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.expect("accept");
            let ws = accept_async(tcp).await.expect("server handshake");
            let (mut server_sink, _server_stream) = ws.split();
            server_sink
                .send(Message::Binary(Bytes::from_static(b"not a moo message")))
                .await
                .expect("send junk");
        });

        let (_outbound_tx, outbound_rx) = mpsc::unbounded_channel();
        let (inbound_tx, _inbound_rx) = mpsc::unbounded_channel();
        let (_stop_tx, stop_rx) = watch::channel(false);

        let result = tokio::time::timeout(
            Duration::from_secs(5),
            run(
                addr,
                Duration::from_secs(5),
                Duration::from_secs(30),
                outbound_rx,
                inbound_tx,
                stop_rx,
            ),
        )
        .await
        .expect("run finishes before timeout");

        assert!(matches!(result, Err(TransportError::Framing(_))));
        server.await.expect("server task");
    }

    #[tokio::test]
    async fn empty_binary_frame_is_ignored_as_a_keepalive() {
        let (addr, listener) = bind_mock_server().await;

        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.expect("accept");
            let ws = accept_async(tcp).await.expect("server handshake");
            let (mut server_sink, _server_stream) = ws.split();

            // A zero-length binary frame, followed by a real message — the empty frame must
            // not end the connection or otherwise stop the real one from arriving.
            server_sink
                .send(Message::Binary(Bytes::new()))
                .await
                .expect("send empty frame");
            let reply = MooMessage {
                verb: MooVerb::Complete,
                name: "Success".to_string(),
                request_id: 1,
                headers: HashMap::new(),
                body: None,
            };
            server_sink
                .send(Message::Binary(Bytes::from(reply.encode())))
                .await
                .expect("send reply");
        });

        let (_outbound_tx, outbound_rx) = mpsc::unbounded_channel();
        let (inbound_tx, mut inbound_rx) = mpsc::unbounded_channel();
        let (_stop_tx, stop_rx) = watch::channel(false);

        let _client = tokio::spawn(run(
            addr,
            Duration::from_secs(5),
            Duration::from_secs(30),
            outbound_rx,
            inbound_tx,
            stop_rx,
        ));

        let reply = tokio::time::timeout(Duration::from_secs(5), inbound_rx.recv())
            .await
            .expect("reply arrives before timeout")
            .expect("reply channel open");
        assert_eq!(reply.name, "Success");
        assert_eq!(reply.request_id, 1);

        server.await.expect("server task");
    }

    #[tokio::test]
    async fn missed_pong_closes_connection() {
        let (addr, listener) = bind_mock_server().await;

        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.expect("accept");
            // Accept the handshake, then never read or write again — simulates an
            // unresponsive Core, so no pong ever comes back for the client's pings.
            let _ws = accept_async(tcp).await.expect("server handshake");
            tokio::time::sleep(Duration::from_secs(5)).await;
        });

        let (_outbound_tx, outbound_rx) = mpsc::unbounded_channel();
        let (inbound_tx, _inbound_rx) = mpsc::unbounded_channel();
        let (_stop_tx, stop_rx) = watch::channel(false);

        let result = tokio::time::timeout(
            Duration::from_secs(5),
            run(
                addr,
                Duration::from_secs(5),
                Duration::from_millis(20),
                outbound_rx,
                inbound_tx,
                stop_rx,
            ),
        )
        .await
        .expect("run finishes before timeout");

        assert!(matches!(result, Err(TransportError::PongTimeout(_))));
        server.abort();
    }

    #[tokio::test]
    async fn connect_times_out_when_the_peer_never_completes_the_ws_upgrade() {
        let (addr, listener) = bind_mock_server().await;

        let server = tokio::spawn(async move {
            let (_tcp, _) = listener.accept().await.expect("accept");
            // Accept the raw TCP connection but never speak the WS upgrade at all — simulates a
            // reachable-but-not-actually-Roon address (e.g. a stale cached address some other
            // service now occupies), as distinct from `missed_pong_closes_connection`'s
            // already-connected-but-unresponsive-Core case.
            tokio::time::sleep(Duration::from_secs(5)).await;
        });

        let (_outbound_tx, outbound_rx) = mpsc::unbounded_channel();
        let (inbound_tx, _inbound_rx) = mpsc::unbounded_channel();
        let (_stop_tx, stop_rx) = watch::channel(false);

        let result = tokio::time::timeout(
            Duration::from_secs(5),
            run(
                addr,
                Duration::from_millis(50),
                Duration::from_secs(30),
                outbound_rx,
                inbound_tx,
                stop_rx,
            ),
        )
        .await
        .expect("run finishes before the outer test timeout");

        assert!(matches!(
            result,
            Err(TransportError::ConnectTimeout { timeout, .. }) if timeout == Duration::from_millis(50)
        ));
        server.abort();
    }

    #[tokio::test]
    async fn probe_succeeds_against_a_live_ws_server() {
        let (addr, listener) = bind_mock_server().await;

        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.expect("accept");
            let _ws = accept_async(tcp).await.expect("server handshake");
            // Nothing else to do — `probe` closes its side right after connecting.
        });

        assert!(probe(addr, Duration::from_secs(5)).await);
        server.await.expect("server task");
    }

    #[tokio::test]
    async fn probe_fails_when_the_peer_never_completes_the_ws_upgrade() {
        let (addr, listener) = bind_mock_server().await;

        let server = tokio::spawn(async move {
            let (_tcp, _) = listener.accept().await.expect("accept");
            tokio::time::sleep(Duration::from_secs(5)).await;
        });

        assert!(!probe(addr, Duration::from_millis(50)).await);
        server.abort();
    }
}
