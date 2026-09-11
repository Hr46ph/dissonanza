//! MOO registry registration handshake (`com.roonlabs.registry:1/info` then
//! `.../register`) and the `com.roonlabs.pairing:1`/`com.roonlabs.ping:1` services this
//! extension provides in response, per `docs/protocol/sood-moo.md`.
//!
//! Operates purely over the outbound/inbound [`MooMessage`] channels `moo::transport` already
//! exposes — independently testable without a real websocket. Driven by `connection/mod.rs`.

use std::collections::HashMap;

use tokio::sync::mpsc;

use super::super::config::ConnectionConfig;
use super::message::{MooBody, MooMessage, MooVerb};

#[derive(Debug, thiserror::Error)]
pub enum HandshakeError {
    #[error("connection closed before the {0} step completed")]
    ConnectionClosed(&'static str),
    #[error("registry:1/register failed: {0}")]
    RegistrationFailed(String),
    #[error("Registered response had no body")]
    MissingBody,
    #[error("Registered response body was binary, not JSON")]
    NonJsonBody,
    #[error("malformed Registered body: {0}")]
    MalformedBody(#[from] serde_json::Error),
}

/// The `COMPLETE Registered` body, per `docs/protocol/sood-moo.md`. Its `provided_services` is
/// the Core's own list of what it offers — a distinct meaning from the `provided_services` this
/// extension declares going in.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub(crate) struct Registered {
    pub core_id: String,
    pub token: String,
    pub display_name: String,
    pub display_version: String,
    pub provided_services: Vec<String>,
}

/// Runs the registration handshake to completion: `registry:1/info` (its response body carries
/// a `core_id` used to look up a saved pairing token — no cache/persistence exists yet this
/// phase, so `saved_token` is passed in directly by the caller instead) then
/// `registry:1/register` declaring `required_services`/`optional_services`/`provided_services`,
/// returning the parsed `Registered` body on success.
pub(crate) async fn register(
    outbound_tx: &mpsc::UnboundedSender<MooMessage>,
    inbound_rx: &mut mpsc::UnboundedReceiver<MooMessage>,
    config: &ConnectionConfig,
    required_services: &[&str],
    optional_services: &[&str],
    provided_services: &[&str],
    saved_token: Option<&str>,
) -> Result<Registered, HandshakeError> {
    send_request(outbound_tx, 1, "com.roonlabs.registry:1/info", None)?;
    recv_response(outbound_tx, inbound_rx, 1, false, "registry:1/info").await?;

    let mut body = serde_json::json!({
        "extension_id": config.extension_id,
        "display_name": config.display_name,
        "display_version": config.display_version,
        "publisher": config.publisher,
        "email": config.email,
        "required_services": required_services,
        "optional_services": optional_services,
        "provided_services": provided_services,
    });
    if let Some(website) = &config.website {
        body["website"] = serde_json::Value::String(website.clone());
    }
    if let Some(token) = saved_token {
        body["token"] = serde_json::Value::String(token.to_string());
    }

    send_request(
        outbound_tx,
        2,
        "com.roonlabs.registry:1/register",
        Some(body),
    )?;
    // The Core answers this one with a `CONTINUE`, not a `COMPLETE` — confirmed against a live
    // Core, see docs/protocol/sood-moo.md's step 4. A client that only accepted `COMPLETE` here
    // would hang forever even once the ping-during-handshake issue below is handled.
    let response = recv_response(outbound_tx, inbound_rx, 2, true, "registry:1/register").await?;
    if response.name != "Registered" {
        return Err(HandshakeError::RegistrationFailed(response.name));
    }

    let body = match response.body {
        Some(MooBody::Json(value)) => value,
        Some(MooBody::Binary { .. }) => return Err(HandshakeError::NonJsonBody),
        None => return Err(HandshakeError::MissingBody),
    };
    Ok(serde_json::from_value(body)?)
}

/// Name of the `com.roonlabs.pairing:1` service this extension must provide (already declared
/// in `register`'s `provided_services`) so the Core has somewhere to send its pairing
/// notification.
pub(crate) const PAIRING_SERVICE: &str = "com.roonlabs.pairing:1";

/// A change in this connection's pairing status. There is no `Unpaired` counterpart here: per
/// `node-roon-api`'s reference implementation, unpairing has no wire message of its own — the
/// Core signals it only by closing the connection, which [`PairingState`] never sees. That
/// disconnect-inferred, known-unreliable path is exactly CLAUDE.md §1's keepalive backstop,
/// handled where the connection lifecycle itself is tracked, not here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PairingEvent {
    Paired { core_id: String },
}

/// State for the `com.roonlabs.pairing:1` service, mirroring `node-roon-api`'s `pairing.js`:
/// `subscribe_pairing`/`get_pairing` report the current pairing status, and an inbound `pair`
/// request (sent by the Core when the user pairs this extension in Roon's UI) sets it. A single
/// MOO connection only ever talks to one Core, so pairing here is a plain yes/no against that
/// one `core_id` — no per-core bookkeeping needed.
#[derive(Debug, Default)]
pub(crate) struct PairingState {
    paired: bool,
    /// The request id of an open `subscribe_pairing` subscription, if any — answered with a
    /// `Changed` CONTINUE when `paired` flips. At most one subscriber: the Core this connection
    /// belongs to.
    subscriber: Option<u32>,
}

impl PairingState {
    /// Handles one inbound `REQUEST` addressed to [`PAIRING_SERVICE`], replying over
    /// `outbound_tx` as the wire protocol requires and returning a [`PairingEvent`] the first
    /// time a `pair` request arrives. `core_id` is this connection's own Core (from the earlier
    /// `Registered` body) — the `pair` request's body carries no core identity of its own to
    /// check against, since on a single-Core connection it can only ever mean "this one".
    pub(crate) fn handle_request(
        &mut self,
        outbound_tx: &mpsc::UnboundedSender<MooMessage>,
        core_id: &str,
        request: &MooMessage,
    ) -> Result<Option<PairingEvent>, HandshakeError> {
        match request.name.as_str() {
            "com.roonlabs.pairing:1/subscribe_pairing" => {
                self.subscriber = Some(request.request_id);
                send_continue(
                    outbound_tx,
                    request.request_id,
                    "Subscribed",
                    Some(paired_core_id_body(self.paired, core_id)),
                )?;
                Ok(None)
            }
            "com.roonlabs.pairing:1/unsubscribe_pairing" => {
                self.subscriber = None;
                send_complete(outbound_tx, request.request_id, "Unsubscribed", None)?;
                Ok(None)
            }
            "com.roonlabs.pairing:1/get_pairing" => {
                send_complete(
                    outbound_tx,
                    request.request_id,
                    "Success",
                    Some(paired_core_id_body(self.paired, core_id)),
                )?;
                Ok(None)
            }
            // No COMPLETE is sent here, matching node-roon-api: the reference implementation
            // never answers a `pair` request either, and Roon Cores don't wait on one.
            "com.roonlabs.pairing:1/pair" => {
                if self.paired {
                    return Ok(None);
                }
                self.paired = true;
                if let Some(subscriber) = self.subscriber {
                    send_continue(
                        outbound_tx,
                        subscriber,
                        "Changed",
                        Some(paired_core_id_body(true, core_id)),
                    )?;
                }
                Ok(Some(PairingEvent::Paired {
                    core_id: core_id.to_string(),
                }))
            }
            other => {
                send_complete(
                    outbound_tx,
                    request.request_id,
                    "InvalidRequest",
                    Some(serde_json::json!({ "error": format!("unknown request name: {other}") })),
                )?;
                Ok(None)
            }
        }
    }
}

/// Name of the `com.roonlabs.ping:1` service this extension must provide (already declared in
/// `register`'s `provided_services`) so the Core can verify liveness at the MOO-message level,
/// per `docs/protocol/sood-moo.md`. Distinct from the WS-level ping/pong `moo::transport` already
/// runs — this is an application-level request the Core sends over an established connection.
pub(crate) const PING_SERVICE: &str = "com.roonlabs.ping:1";

/// Handles one inbound `REQUEST` addressed to [`PING_SERVICE`], replying `COMPLETE Success`.
/// Stateless — unlike pairing, there's nothing to track between calls.
pub(crate) fn handle_ping_request(
    outbound_tx: &mpsc::UnboundedSender<MooMessage>,
    request: &MooMessage,
) -> Result<(), HandshakeError> {
    match request.name.as_str() {
        "com.roonlabs.ping:1/ping" => {
            send_complete(outbound_tx, request.request_id, "Success", None)
        }
        other => send_complete(
            outbound_tx,
            request.request_id,
            "InvalidRequest",
            Some(serde_json::json!({ "error": format!("unknown request name: {other}") })),
        ),
    }
}

fn paired_core_id_body(paired: bool, core_id: &str) -> serde_json::Value {
    serde_json::json!({
        "paired_core_id": if paired { serde_json::Value::String(core_id.to_string()) } else { serde_json::Value::Null },
    })
}

fn send_request(
    outbound_tx: &mpsc::UnboundedSender<MooMessage>,
    request_id: u32,
    name: &'static str,
    body: Option<serde_json::Value>,
) -> Result<(), HandshakeError> {
    send(outbound_tx, MooVerb::Request, request_id, name, body)
}

fn send_continue(
    outbound_tx: &mpsc::UnboundedSender<MooMessage>,
    request_id: u32,
    name: &'static str,
    body: Option<serde_json::Value>,
) -> Result<(), HandshakeError> {
    send(outbound_tx, MooVerb::Continue, request_id, name, body)
}

fn send_complete(
    outbound_tx: &mpsc::UnboundedSender<MooMessage>,
    request_id: u32,
    name: &'static str,
    body: Option<serde_json::Value>,
) -> Result<(), HandshakeError> {
    send(outbound_tx, MooVerb::Complete, request_id, name, body)
}

fn send(
    outbound_tx: &mpsc::UnboundedSender<MooMessage>,
    verb: MooVerb,
    request_id: u32,
    name: &'static str,
    body: Option<serde_json::Value>,
) -> Result<(), HandshakeError> {
    outbound_tx
        .send(MooMessage {
            verb,
            name: name.to_string(),
            request_id,
            headers: HashMap::new(),
            body: body.map(MooBody::Json),
        })
        .map_err(|_| HandshakeError::ConnectionClosed(name))
}

/// Waits for the `COMPLETE` matching `request_id` (or, if `accept_continue`, the first
/// `CONTINUE` too — `registry:1/register` answers with `CONTINUE Registered`, not `COMPLETE`,
/// per docs/protocol/sood-moo.md's step 4). Along the way, answers any inbound
/// `com.roonlabs.ping:1/ping` REQUEST inline via [`handle_ping_request`] instead of discarding
/// it: the Core is observed sending these *during* the handshake itself, before either step's
/// own response arrives, and leaving one unanswered gets the connection reset within seconds.
/// Any other inbound `REQUEST` (e.g. a `pair` call arriving unusually early) and any
/// `CONTINUE`/`COMPLETE` for an unrelated request id are ignored — the two directions assign ids
/// independently, so a collision is possible.
async fn recv_response(
    outbound_tx: &mpsc::UnboundedSender<MooMessage>,
    inbound_rx: &mut mpsc::UnboundedReceiver<MooMessage>,
    request_id: u32,
    accept_continue: bool,
    step: &'static str,
) -> Result<MooMessage, HandshakeError> {
    loop {
        let msg = inbound_rx
            .recv()
            .await
            .ok_or(HandshakeError::ConnectionClosed(step))?;
        if msg.request_id == request_id
            && (msg.verb == MooVerb::Complete || (accept_continue && msg.verb == MooVerb::Continue))
        {
            return Ok(msg);
        }
        if msg.verb == MooVerb::Request && msg.name == "com.roonlabs.ping:1/ping" {
            handle_ping_request(outbound_tx, &msg)?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> ConnectionConfig {
        ConnectionConfig {
            extension_id: "com.example.dissonanza".to_string(),
            display_name: "Dissonanza".to_string(),
            display_version: "0.1.0".to_string(),
            publisher: "two2e32a".to_string(),
            email: "dev@example.com".to_string(),
            website: None,
        }
    }

    fn complete(name: &str, request_id: u32, body: Option<MooBody>) -> MooMessage {
        MooMessage {
            verb: MooVerb::Complete,
            name: name.to_string(),
            request_id,
            headers: HashMap::new(),
            body,
        }
    }

    #[tokio::test]
    async fn info_then_register_declares_provided_services_and_parses_registered_body() {
        let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<MooMessage>();
        let (inbound_tx, mut inbound_rx) = mpsc::unbounded_channel::<MooMessage>();

        let server = tokio::spawn(async move {
            let info = outbound_rx.recv().await.expect("info request");
            assert_eq!(info.name, "com.roonlabs.registry:1/info");
            inbound_tx
                .send(complete("Success", info.request_id, None))
                .expect("reply to info");

            let register = outbound_rx.recv().await.expect("register request");
            assert_eq!(register.name, "com.roonlabs.registry:1/register");
            let body = match register.body {
                Some(MooBody::Json(value)) => value,
                other => panic!("expected a JSON body, got {other:?}"),
            };
            assert_eq!(
                body["required_services"],
                serde_json::json!(["com.roonlabs.transport:2"])
            );
            assert_eq!(
                body["optional_services"],
                serde_json::json!(["com.roonlabs.browse:1"])
            );
            assert_eq!(
                body["provided_services"],
                serde_json::json!(["com.roonlabs.pairing:1", "com.roonlabs.ping:1"])
            );

            inbound_tx
                .send(complete(
                    "Registered",
                    register.request_id,
                    Some(MooBody::Json(serde_json::json!({
                        "core_id": "core-1",
                        "token": "tok-1",
                        "display_name": "Roon Core",
                        "display_version": "2.0",
                        "provided_services": ["com.roonlabs.transport:2"],
                    }))),
                ))
                .expect("reply to register");
        });

        let registered = register(
            &outbound_tx,
            &mut inbound_rx,
            &config(),
            &["com.roonlabs.transport:2"],
            &["com.roonlabs.browse:1"],
            &["com.roonlabs.pairing:1", "com.roonlabs.ping:1"],
            None,
        )
        .await
        .expect("registration succeeds");

        assert_eq!(
            registered,
            Registered {
                core_id: "core-1".to_string(),
                token: "tok-1".to_string(),
                display_name: "Roon Core".to_string(),
                display_version: "2.0".to_string(),
                provided_services: vec!["com.roonlabs.transport:2".to_string()],
            }
        );

        server.await.expect("server task");
    }

    #[tokio::test]
    async fn graceful_failure_on_invalid_request() {
        let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<MooMessage>();
        let (inbound_tx, mut inbound_rx) = mpsc::unbounded_channel::<MooMessage>();

        let server = tokio::spawn(async move {
            let info = outbound_rx.recv().await.expect("info request");
            inbound_tx
                .send(complete("Success", info.request_id, None))
                .expect("reply to info");

            let register = outbound_rx.recv().await.expect("register request");
            inbound_tx
                .send(complete("InvalidRequest", register.request_id, None))
                .expect("reply to register");
        });

        let result = register(
            &outbound_tx,
            &mut inbound_rx,
            &config(),
            &[],
            &[],
            &[],
            None,
        )
        .await;

        assert!(matches!(
            result,
            Err(HandshakeError::RegistrationFailed(name)) if name == "InvalidRequest"
        ));

        server.await.expect("server task");
    }

    #[test]
    fn parses_registered_body_ignoring_unknown_fields() {
        let value = serde_json::json!({
            "core_id": "core-1",
            "token": "tok-1",
            "display_name": "Roon Core",
            "display_version": "2.0",
            "provided_services": ["com.roonlabs.transport:2"],
            "extra_unknown_field": "ignored",
        });

        let registered: Registered = serde_json::from_value(value).expect("parses");

        assert_eq!(
            registered,
            Registered {
                core_id: "core-1".to_string(),
                token: "tok-1".to_string(),
                display_name: "Roon Core".to_string(),
                display_version: "2.0".to_string(),
                provided_services: vec!["com.roonlabs.transport:2".to_string()],
            }
        );
    }

    #[tokio::test]
    async fn includes_saved_token_and_website_when_present() {
        let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<MooMessage>();
        let (inbound_tx, mut inbound_rx) = mpsc::unbounded_channel::<MooMessage>();

        let mut cfg = config();
        cfg.website = Some("https://example.com".to_string());

        let server = tokio::spawn(async move {
            let info = outbound_rx.recv().await.expect("info request");
            inbound_tx
                .send(complete("Success", info.request_id, None))
                .expect("reply to info");

            let register = outbound_rx.recv().await.expect("register request");
            let body = match register.body {
                Some(MooBody::Json(value)) => value,
                other => panic!("expected a JSON body, got {other:?}"),
            };
            assert_eq!(body["website"], "https://example.com");
            assert_eq!(body["token"], "saved-token");

            inbound_tx
                .send(complete(
                    "Registered",
                    register.request_id,
                    Some(MooBody::Json(serde_json::json!({
                        "core_id": "core-1",
                        "token": "tok-1",
                        "display_name": "Roon Core",
                        "display_version": "2.0",
                        "provided_services": [],
                    }))),
                ))
                .expect("reply to register");
        });

        register(
            &outbound_tx,
            &mut inbound_rx,
            &cfg,
            &[],
            &[],
            &[],
            Some("saved-token"),
        )
        .await
        .expect("registration succeeds");

        server.await.expect("server task");
    }

    #[tokio::test]
    async fn accepts_a_continue_as_the_register_response() {
        let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<MooMessage>();
        let (inbound_tx, mut inbound_rx) = mpsc::unbounded_channel::<MooMessage>();

        let server = tokio::spawn(async move {
            let info = outbound_rx.recv().await.expect("info request");
            inbound_tx
                .send(complete("Success", info.request_id, None))
                .expect("reply to info");

            let register = outbound_rx.recv().await.expect("register request");
            // A `CONTINUE`, not a `COMPLETE` — the real Core's actual behavior for this step.
            inbound_tx
                .send(MooMessage {
                    verb: MooVerb::Continue,
                    name: "Registered".to_string(),
                    request_id: register.request_id,
                    headers: HashMap::new(),
                    body: Some(MooBody::Json(serde_json::json!({
                        "core_id": "core-1",
                        "token": "tok-1",
                        "display_name": "Roon Core",
                        "display_version": "2.0",
                        "provided_services": [],
                    }))),
                })
                .expect("reply to register");
        });

        let registered = register(
            &outbound_tx,
            &mut inbound_rx,
            &config(),
            &[],
            &[],
            &[],
            None,
        )
        .await
        .expect("registration succeeds despite a CONTINUE reply");

        assert_eq!(registered.core_id, "core-1");
        server.await.expect("server task");
    }

    #[tokio::test]
    async fn answers_a_ping_request_that_arrives_before_registration_completes() {
        let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<MooMessage>();
        let (inbound_tx, mut inbound_rx) = mpsc::unbounded_channel::<MooMessage>();

        let server = tokio::spawn(async move {
            let info = outbound_rx.recv().await.expect("info request");
            inbound_tx
                .send(complete("Success", info.request_id, None))
                .expect("reply to info");

            let register = outbound_rx.recv().await.expect("register request");

            // The Core sends a ping *during* the handshake, before register's own response.
            inbound_tx
                .send(pairing_request("com.roonlabs.ping:1/ping", 100))
                .expect("send ping request");

            let ping_reply = outbound_rx.recv().await.expect("a reply to the ping");
            assert_eq!(ping_reply.verb, MooVerb::Complete);
            assert_eq!(ping_reply.name, "Success");
            assert_eq!(ping_reply.request_id, 100);

            inbound_tx
                .send(complete(
                    "Registered",
                    register.request_id,
                    Some(MooBody::Json(serde_json::json!({
                        "core_id": "core-1",
                        "token": "tok-1",
                        "display_name": "Roon Core",
                        "display_version": "2.0",
                        "provided_services": [],
                    }))),
                ))
                .expect("reply to register");
        });

        let registered = register(
            &outbound_tx,
            &mut inbound_rx,
            &config(),
            &[],
            &[],
            &[],
            None,
        )
        .await
        .expect("registration succeeds despite the interleaved ping");

        assert_eq!(registered.core_id, "core-1");
        server.await.expect("server task");
    }

    fn pairing_request(name: &str, request_id: u32) -> MooMessage {
        MooMessage {
            verb: MooVerb::Request,
            name: name.to_string(),
            request_id,
            headers: HashMap::new(),
            body: None,
        }
    }

    #[test]
    fn subscribe_pairing_reports_null_before_pairing() {
        let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<MooMessage>();
        let mut state = PairingState::default();

        let event = state
            .handle_request(
                &outbound_tx,
                "core-1",
                &pairing_request("com.roonlabs.pairing:1/subscribe_pairing", 5),
            )
            .expect("handled");
        assert_eq!(event, None);

        let reply = outbound_rx.try_recv().expect("a reply was sent");
        assert_eq!(reply.verb, MooVerb::Continue);
        assert_eq!(reply.name, "Subscribed");
        assert_eq!(reply.request_id, 5);
        assert_eq!(
            reply.body,
            Some(MooBody::Json(serde_json::json!({ "paired_core_id": null })))
        );
    }

    #[test]
    fn pair_request_emits_paired_event_and_notifies_subscriber() {
        let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<MooMessage>();
        let mut state = PairingState::default();

        state
            .handle_request(
                &outbound_tx,
                "core-1",
                &pairing_request("com.roonlabs.pairing:1/subscribe_pairing", 5),
            )
            .expect("handled");
        outbound_rx.try_recv().expect("Subscribed reply"); // drain it

        let event = state
            .handle_request(
                &outbound_tx,
                "core-1",
                &pairing_request("com.roonlabs.pairing:1/pair", 6),
            )
            .expect("handled");
        assert_eq!(
            event,
            Some(PairingEvent::Paired {
                core_id: "core-1".to_string()
            })
        );

        // Notifies the still-open subscription, addressed by its own request id — not the
        // `pair` request's.
        let notification = outbound_rx.try_recv().expect("a Changed notification");
        assert_eq!(notification.verb, MooVerb::Continue);
        assert_eq!(notification.name, "Changed");
        assert_eq!(notification.request_id, 5);
        assert_eq!(
            notification.body,
            Some(MooBody::Json(
                serde_json::json!({ "paired_core_id": "core-1" })
            ))
        );

        // No COMPLETE for the `pair` request itself — matches node-roon-api, which never
        // answers one either.
        assert!(outbound_rx.try_recv().is_err());
    }

    #[test]
    fn pair_request_is_idempotent_once_paired() {
        let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<MooMessage>();
        let mut state = PairingState::default();

        state
            .handle_request(
                &outbound_tx,
                "core-1",
                &pairing_request("com.roonlabs.pairing:1/pair", 1),
            )
            .expect("handled");
        let second = state
            .handle_request(
                &outbound_tx,
                "core-1",
                &pairing_request("com.roonlabs.pairing:1/pair", 2),
            )
            .expect("handled");

        assert_eq!(second, None, "already paired, second pair is a no-op");
        assert!(
            outbound_rx.try_recv().is_err(),
            "no subscriber was ever registered, so nothing should have been sent"
        );
    }

    #[test]
    fn get_pairing_reports_core_id_once_paired() {
        let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<MooMessage>();
        let mut state = PairingState::default();

        state
            .handle_request(
                &outbound_tx,
                "core-1",
                &pairing_request("com.roonlabs.pairing:1/pair", 1),
            )
            .expect("handled");

        state
            .handle_request(
                &outbound_tx,
                "core-1",
                &pairing_request("com.roonlabs.pairing:1/get_pairing", 2),
            )
            .expect("handled");

        let reply = outbound_rx.try_recv().expect("a reply was sent");
        assert_eq!(reply.verb, MooVerb::Complete);
        assert_eq!(reply.name, "Success");
        assert_eq!(reply.request_id, 2);
        assert_eq!(
            reply.body,
            Some(MooBody::Json(
                serde_json::json!({ "paired_core_id": "core-1" })
            ))
        );
    }

    #[test]
    fn unsubscribe_pairing_stops_future_change_notifications() {
        let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<MooMessage>();
        let mut state = PairingState::default();

        state
            .handle_request(
                &outbound_tx,
                "core-1",
                &pairing_request("com.roonlabs.pairing:1/subscribe_pairing", 5),
            )
            .expect("handled");
        outbound_rx.try_recv().expect("Subscribed reply");

        state
            .handle_request(
                &outbound_tx,
                "core-1",
                &pairing_request("com.roonlabs.pairing:1/unsubscribe_pairing", 7),
            )
            .expect("handled");
        let unsubscribed = outbound_rx.try_recv().expect("Unsubscribed reply");
        assert_eq!(unsubscribed.verb, MooVerb::Complete);
        assert_eq!(unsubscribed.name, "Unsubscribed");
        assert_eq!(unsubscribed.request_id, 7);

        state
            .handle_request(
                &outbound_tx,
                "core-1",
                &pairing_request("com.roonlabs.pairing:1/pair", 8),
            )
            .expect("handled");

        assert!(
            outbound_rx.try_recv().is_err(),
            "no subscriber left to notify"
        );
    }

    #[test]
    fn ping_request_gets_success() {
        let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<MooMessage>();

        handle_ping_request(
            &outbound_tx,
            &pairing_request("com.roonlabs.ping:1/ping", 3),
        )
        .expect("handled");

        let reply = outbound_rx.try_recv().expect("a reply was sent");
        assert_eq!(reply.verb, MooVerb::Complete);
        assert_eq!(reply.name, "Success");
        assert_eq!(reply.request_id, 3);
        assert_eq!(reply.body, None);
    }

    #[test]
    fn unknown_ping_request_gets_invalid_request() {
        let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<MooMessage>();

        handle_ping_request(
            &outbound_tx,
            &pairing_request("com.roonlabs.ping:1/frobnicate", 4),
        )
        .expect("handled");

        let reply = outbound_rx.try_recv().expect("a reply was sent");
        assert_eq!(reply.verb, MooVerb::Complete);
        assert_eq!(reply.name, "InvalidRequest");
        assert_eq!(reply.request_id, 4);
    }

    #[test]
    fn unknown_pairing_request_gets_invalid_request() {
        let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<MooMessage>();
        let mut state = PairingState::default();

        let event = state
            .handle_request(
                &outbound_tx,
                "core-1",
                &pairing_request("com.roonlabs.pairing:1/frobnicate", 9),
            )
            .expect("handled");
        assert_eq!(event, None);

        let reply = outbound_rx.try_recv().expect("a reply was sent");
        assert_eq!(reply.verb, MooVerb::Complete);
        assert_eq!(reply.name, "InvalidRequest");
        assert_eq!(reply.request_id, 9);
    }
}
