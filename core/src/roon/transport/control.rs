//! `control` and `seek` — one-shot playback-control verbs against a zone or output, per
//! docs/protocol/transport.md's "Control verbs" section. Built entirely on
//! `connection::ConnectionRequests`, same as `zones.rs` — never touches SOOD discovery, pairing,
//! or reconnect itself.

use serde::Serialize;

use crate::roon::connection::{ConnectionRequests, MooMessage, MooResponseStream, MooVerb};

use super::error::TransportError;

const CONTROL: &str = "com.roonlabs.transport:2/control";
const SEEK: &str = "com.roonlabs.transport:2/seek";

/// `control ∈ {play, pause, playpause, stop, previous, next}`, per docs/protocol/transport.md.
/// Check the corresponding `is_*_allowed` field on `Zone` before offering one of these in a UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlAction {
    Play,
    Pause,
    #[serde(rename = "playpause")]
    PlayPause,
    Stop,
    Previous,
    Next,
}

/// `how ∈ {relative, absolute}` for `seek`, per docs/protocol/transport.md.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SeekHow {
    Relative,
    Absolute,
}

/// Sends `control` against `zone_or_output_id` and waits for its `COMPLETE`. Available as soon as
/// the registry handshake completes, same as `subscribe_zones` — not gated on `Paired`.
pub async fn control(
    requests: &ConnectionRequests,
    zone_or_output_id: &str,
    action: ControlAction,
) -> Result<(), TransportError> {
    let body = serde_json::json!({
        "zone_or_output_id": zone_or_output_id,
        "control": action,
    });
    let mut stream = requests.send_request(CONTROL, Some(body))?;
    await_command_response(CONTROL, &mut stream).await
}

/// Sends `seek` against `zone_or_output_id` and waits for its `COMPLETE`.
pub async fn seek(
    requests: &ConnectionRequests,
    zone_or_output_id: &str,
    how: SeekHow,
    seconds: f64,
) -> Result<(), TransportError> {
    let body = serde_json::json!({
        "zone_or_output_id": zone_or_output_id,
        "how": how,
        "seconds": seconds,
    });
    let mut stream = requests.send_request(SEEK, Some(body))?;
    await_command_response(SEEK, &mut stream).await
}

/// Waits for the single `COMPLETE` a control verb replies with, per docs/protocol/transport.md
/// ("get back a single `COMPLETE` (`Success`, or an error name, no `CONTINUE`s)"). `request_name`
/// only names the request in `NoResponse` if the stream ends before anything arrives.
async fn await_command_response(
    request_name: &str,
    stream: &mut MooResponseStream,
) -> Result<(), TransportError> {
    let msg = stream
        .recv()
        .await
        .ok_or_else(|| TransportError::NoResponse {
            name: request_name.to_string(),
        })?;
    parse_command_response(msg)
}

/// Pure parsing half of [`await_command_response`], split out so it can be unit-tested directly
/// against fabricated [`MooMessage`]s, mirroring `zones.rs`'s `parse_zone_event`.
fn parse_command_response(msg: MooMessage) -> Result<(), TransportError> {
    if msg.verb != MooVerb::Complete {
        return Err(TransportError::UnexpectedResponse {
            verb: msg.verb,
            name: msg.name,
        });
    }
    if msg.name == "Success" {
        Ok(())
    } else {
        Err(TransportError::CommandFailed { name: msg.name })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tokio::sync::mpsc;

    use super::*;

    fn complete_msg(name: &str) -> MooMessage {
        MooMessage {
            verb: MooVerb::Complete,
            name: name.to_string(),
            request_id: 3,
            headers: HashMap::new(),
            body: None,
        }
    }

    #[test]
    fn parses_success_as_ok() {
        assert!(parse_command_response(complete_msg("Success")).is_ok());
    }

    #[test]
    fn parses_a_non_success_complete_as_command_failed() {
        assert!(matches!(
            parse_command_response(complete_msg("InvalidRequest")),
            Err(TransportError::CommandFailed { name }) if name == "InvalidRequest"
        ));
    }

    #[test]
    fn rejects_a_continue_as_unexpected() {
        let msg = MooMessage {
            verb: MooVerb::Continue,
            ..complete_msg("Success")
        };

        assert!(matches!(
            parse_command_response(msg),
            Err(TransportError::UnexpectedResponse { verb: MooVerb::Continue, name })
                if name == "Success"
        ));
    }

    #[tokio::test]
    async fn await_command_response_forwards_the_first_message() {
        let (tx, rx) = mpsc::unbounded_channel::<MooMessage>();
        let mut stream = MooResponseStream::new(rx);
        tx.send(complete_msg("Success")).expect("channel open");

        assert!(await_command_response(CONTROL, &mut stream).await.is_ok());
    }

    #[tokio::test]
    async fn await_command_response_reports_no_response_when_the_stream_ends_empty() {
        let (tx, rx) = mpsc::unbounded_channel::<MooMessage>();
        let mut stream = MooResponseStream::new(rx);
        drop(tx);

        assert!(matches!(
            await_command_response(CONTROL, &mut stream).await,
            Err(TransportError::NoResponse { name }) if name == CONTROL
        ));
    }

    #[test]
    fn control_action_serializes_to_wire_names() {
        assert_eq!(
            serde_json::to_value(ControlAction::PlayPause).unwrap(),
            serde_json::json!("playpause")
        );
        assert_eq!(
            serde_json::to_value(ControlAction::Previous).unwrap(),
            serde_json::json!("previous")
        );
    }

    #[test]
    fn seek_how_serializes_to_wire_names() {
        assert_eq!(
            serde_json::to_value(SeekHow::Relative).unwrap(),
            serde_json::json!("relative")
        );
    }
}
