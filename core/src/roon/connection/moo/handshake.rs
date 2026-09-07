//! MOO registry registration handshake: `com.roonlabs.registry:1/info` followed by
//! `com.roonlabs.registry:1/register`, per `docs/protocol/sood-moo.md`.
//!
//! Operates purely over the outbound/inbound [`MooMessage`] channels `moo::transport` already
//! exposes — independently testable without a real websocket. Handling inbound `pair`/`unpair`
//! events and responding to `ping:1` are later steps; this module only gets a registered,
//! token-holding connection off the ground. Not wired into a connection state machine yet, so
//! its items are unused outside their own tests.
#![allow(dead_code)]

use std::collections::HashMap;

use tokio::sync::mpsc;

use super::super::config::ConnectionConfig;
use super::message::{MooBody, MooMessage, MooVerb};

#[derive(Debug, thiserror::Error)]
pub(crate) enum HandshakeError {
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
/// `registry:1/register` declaring `provided_services`, returning the parsed `Registered` body
/// on success.
pub(crate) async fn register(
    outbound_tx: &mpsc::UnboundedSender<MooMessage>,
    inbound_rx: &mut mpsc::UnboundedReceiver<MooMessage>,
    config: &ConnectionConfig,
    provided_services: &[&str],
    saved_token: Option<&str>,
) -> Result<Registered, HandshakeError> {
    send_request(outbound_tx, 1, "com.roonlabs.registry:1/info", None)?;
    recv_complete(inbound_rx, 1, "registry:1/info").await?;

    let mut body = serde_json::json!({
        "extension_id": config.extension_id,
        "display_name": config.display_name,
        "display_version": config.display_version,
        "publisher": config.publisher,
        "email": config.email,
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
    let response = recv_complete(inbound_rx, 2, "registry:1/register").await?;
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

fn send_request(
    outbound_tx: &mpsc::UnboundedSender<MooMessage>,
    request_id: u32,
    name: &'static str,
    body: Option<serde_json::Value>,
) -> Result<(), HandshakeError> {
    outbound_tx
        .send(MooMessage {
            verb: MooVerb::Request,
            name: name.to_string(),
            request_id,
            headers: HashMap::new(),
            body: body.map(MooBody::Json),
        })
        .map_err(|_| HandshakeError::ConnectionClosed(name))
}

/// Waits for the `COMPLETE` matching `request_id`, ignoring any `CONTINUE`s, any inbound
/// `REQUEST`s (Core-initiated calls like `pair`/`ping:1`, handled by later steps), and any
/// `COMPLETE` for an unrelated request id — the two directions assign ids independently, so a
/// collision is possible.
async fn recv_complete(
    inbound_rx: &mut mpsc::UnboundedReceiver<MooMessage>,
    request_id: u32,
    step: &'static str,
) -> Result<MooMessage, HandshakeError> {
    loop {
        let msg = inbound_rx
            .recv()
            .await
            .ok_or(HandshakeError::ConnectionClosed(step))?;
        if msg.verb == MooVerb::Complete && msg.request_id == request_id {
            return Ok(msg);
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

        let result = register(&outbound_tx, &mut inbound_rx, &config(), &[], None).await;

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
            Some("saved-token"),
        )
        .await
        .expect("registration succeeds");

        server.await.expect("server task");
    }
}
