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

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("failed to connect to {addr}: {source}")]
    Connect {
        addr: SocketAddr,
        #[source]
        source: WsError,
    },
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
/// `docs/protocol/sood-moo.md`.
pub(crate) async fn run(
    addr: SocketAddr,
    ping_interval: Duration,
    mut outbound_rx: mpsc::UnboundedReceiver<MooMessage>,
    inbound_tx: mpsc::UnboundedSender<MooMessage>,
    mut stop_rx: watch::Receiver<bool>,
) -> Result<(), TransportError> {
    if *stop_rx.borrow() {
        return Ok(());
    }

    let (ws_stream, _response) = connect_async(format!("ws://{addr}/api"))
        .await
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
}
