//! Generic "send a MOO request, get a response stream back" surface (IMPL_TRANSPORT.md Phase 1),
//! so other `core::roon` modules (`transport`, and future `browse`/`image`) can talk to whichever
//! Core-provided service they need over the already-paired connection, without `connection`
//! needing to know anything about `transport:2` specifically — CLAUDE.md §1 keeps discovery,
//! pairing, keepalive, and reconnect `connection`'s alone, not every service's request/response
//! traffic on top of that connection.
//!
//! [`ConnectionRequests`] is deliberately a separate type from [`super::ConnectionHandle`]: any
//! number of service modules can each hold their own `Clone` of it for the app's whole lifetime,
//! while `ConnectionHandle::shutdown` keeps its single-owner, consuming shape unchanged.

use tokio::sync::{mpsc, watch};

use super::MooMessage;

/// The sending half of the command channel `connection/mod.rs`'s request loop reads from,
/// published through a `watch` cell so [`ConnectionRequests`] always has the current connection's
/// sender (or `None` while none is live). Recreated fresh every reconnect — never reused across
/// one — so a stale clone captured just before a disconnect simply fails to send rather than
/// queuing against a connection that no longer exists.
pub(super) type CommandTx = mpsc::UnboundedSender<ConnectionCommand>;

/// A command sent into the request loop. Not part of the public API — callers only ever see
/// [`ConnectionRequests::send_request`]'s return value, never this type itself.
pub(super) enum ConnectionCommand {
    SendRequest {
        name: String,
        body: Option<serde_json::Value>,
        /// Where to forward every `CONTINUE`/`COMPLETE` sharing the request-id this gets
        /// allocated. Closed by the loop once a `COMPLETE` arrives, or implicitly whenever the
        /// loop itself ends (reconnect or shutdown) — either way, [`MooResponseStream::recv`]
        /// then reports the stream as finished.
        response_tx: mpsc::UnboundedSender<MooMessage>,
    },
}

/// Sending a request over a connection that isn't currently past the registry handshake, or that
/// dropped between snapshotting the current command sender and using it.
#[derive(Debug, thiserror::Error)]
pub enum ConnectionRequestError {
    #[error("not connected to a Roon Core")]
    NotConnected,
}

/// A cheap, `Clone`-able handle for sending MOO requests (e.g. `com.roonlabs.transport:2/
/// subscribe_zones`) over whichever connection is currently paired, per the module doc above.
#[derive(Debug, Clone)]
pub struct ConnectionRequests {
    command_tx: watch::Receiver<Option<CommandTx>>,
}

impl ConnectionRequests {
    pub(super) fn new(command_tx: watch::Receiver<Option<CommandTx>>) -> Self {
        Self { command_tx }
    }

    /// Sends a MOO `REQUEST` for `name` with the given JSON `body` (`None` for no body),
    /// returning a stream of its `CONTINUE`/`COMPLETE` responses. Available as soon as the
    /// registry handshake completes — not gated on `Paired`, since `connection` has no opinion on
    /// whether a given Core-provided service is meaningful to call yet; a caller wanting to wait
    /// for `Paired` enforces that itself by watching [`super::ConnectionEvent`].
    ///
    /// Fails immediately with `NotConnected` rather than queuing if no connection is currently up
    /// to that point, including the moment just after a disconnect — per
    /// `docs/protocol/transport.md`'s reconnect finding, re-issuing a subscription after
    /// reconnecting is always the caller's own responsibility, never automatic here.
    pub fn send_request(
        &self,
        name: impl Into<String>,
        body: Option<serde_json::Value>,
    ) -> Result<MooResponseStream, ConnectionRequestError> {
        let command_tx = self
            .command_tx
            .borrow()
            .clone()
            .ok_or(ConnectionRequestError::NotConnected)?;
        let (response_tx, response_rx) = mpsc::unbounded_channel();
        command_tx
            .send(ConnectionCommand::SendRequest {
                name: name.into(),
                body,
                response_tx,
            })
            .map_err(|_| ConnectionRequestError::NotConnected)?;
        Ok(MooResponseStream::new(response_rx))
    }
}

/// A stream of `CONTINUE`/`COMPLETE` responses to one request sent via
/// [`ConnectionRequests::send_request`]. Ends (`recv` returns `None`) once a `COMPLETE` has been
/// delivered, or as soon as the underlying connection is lost, whichever comes first — a caller
/// still waiting on an open subscription when the connection drops sees its stream end exactly
/// the same way a completed one-shot request does, no separate signal needed to know it must
/// re-subscribe after the next `Paired`.
#[derive(Debug)]
pub struct MooResponseStream {
    rx: mpsc::UnboundedReceiver<MooMessage>,
}

impl MooResponseStream {
    /// Wraps a raw receiver directly. Visible crate-wide (rather than only to
    /// [`ConnectionRequests::send_request`]) so other `core::roon` modules — `transport`, and
    /// future `browse`/`image` — can fabricate one in their own unit tests to drive their
    /// message-parsing logic over plain `mpsc` channels, without needing a live Core, matching how
    /// this crate already tests `moo::handshake` and this module's own tests below.
    pub(crate) fn new(rx: mpsc::UnboundedReceiver<MooMessage>) -> Self {
        Self { rx }
    }

    pub async fn recv(&mut self) -> Option<MooMessage> {
        self.rx.recv().await
    }
}

#[cfg(test)]
mod tests {
    use super::super::MooVerb;
    use super::*;

    #[test]
    fn send_request_fails_when_no_connection_is_up() {
        let (_watch_tx, watch_rx) = watch::channel::<Option<CommandTx>>(None);
        let requests = ConnectionRequests::new(watch_rx);

        let result = requests.send_request("com.roonlabs.transport:2/get_zones", None);

        assert!(matches!(result, Err(ConnectionRequestError::NotConnected)));
    }

    #[test]
    fn send_request_fails_when_the_published_channel_is_already_closed() {
        let (command_tx, command_rx) = mpsc::unbounded_channel::<ConnectionCommand>();
        drop(command_rx); // simulates a reconnect that already ended this connection attempt
        let (_watch_tx, watch_rx) = watch::channel(Some(command_tx));
        let requests = ConnectionRequests::new(watch_rx);

        let result = requests.send_request("com.roonlabs.transport:2/get_zones", None);

        assert!(matches!(result, Err(ConnectionRequestError::NotConnected)));
    }

    #[tokio::test]
    async fn send_request_forwards_the_command_and_streams_responses_back() {
        let (command_tx, mut command_rx) = mpsc::unbounded_channel::<ConnectionCommand>();
        let (_watch_tx, watch_rx) = watch::channel(Some(command_tx));
        let requests = ConnectionRequests::new(watch_rx);
        let body = serde_json::json!({"subscription_key": 0});

        let mut stream = requests
            .send_request(
                "com.roonlabs.transport:2/subscribe_zones",
                Some(body.clone()),
            )
            .expect("a connection is up");

        let ConnectionCommand::SendRequest {
            name,
            body: sent_body,
            response_tx,
        } = command_rx.try_recv().expect("command was sent");
        assert_eq!(name, "com.roonlabs.transport:2/subscribe_zones");
        assert_eq!(sent_body, Some(body));

        response_tx
            .send(MooMessage {
                verb: MooVerb::Continue,
                name: "Subscribed".to_string(),
                request_id: 3,
                headers: std::collections::HashMap::new(),
                body: None,
            })
            .expect("stream is still open");

        let received = stream.recv().await.expect("a response was forwarded");
        assert_eq!(received.name, "Subscribed");

        drop(response_tx);
        assert_eq!(
            stream.recv().await,
            None,
            "stream ends once the sender side is gone"
        );
    }

    #[test]
    fn clones_share_the_same_watched_connection() {
        let (watch_tx, watch_rx) = watch::channel::<Option<CommandTx>>(None);
        let a = ConnectionRequests::new(watch_rx);
        let b = a.clone();

        assert!(matches!(
            b.send_request("x", None),
            Err(ConnectionRequestError::NotConnected)
        ));

        let (command_tx, _command_rx) = mpsc::unbounded_channel::<ConnectionCommand>();
        watch_tx.send_replace(Some(command_tx));

        assert!(
            a.send_request("x", None).is_ok(),
            "both clones see the update"
        );
    }
}
