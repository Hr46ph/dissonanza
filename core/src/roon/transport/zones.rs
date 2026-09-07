//! `subscribe_zones` and the `ZoneEvent` stream it produces, per docs/protocol/transport.md's
//! "Subscription semantics and reconnect" section. Built entirely on
//! `connection::ConnectionRequests` — never touches SOOD discovery, pairing, or reconnect itself.

use serde::Deserialize;

use crate::roon::connection::{
    ConnectionRequests, MooBody, MooMessage, MooResponseStream, MooVerb,
};

use super::error::TransportError;
use super::model::Zone;

const SUBSCRIBE_ZONES: &str = "com.roonlabs.transport:2/subscribe_zones";

/// This app only ever opens one zones subscription (CLAUDE.md's multi-zone/multi-Core non-goal),
/// so a fixed key is sufficient — the Core only uses it to disambiguate multiple concurrent
/// subscriptions of the same kind from one client, per docs/protocol/transport.md.
const SUBSCRIPTION_KEY: u32 = 0;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ZoneSeekChange {
    pub zone_id: String,
    #[serde(default)]
    pub seek_position: Option<f64>,
    pub queue_time_remaining: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ZoneEvent {
    /// The first response to `subscribe_zones`: full current state of every zone.
    Subscribed { zones: Vec<Zone> },
    /// A subsequent delta. Any subset of these can be non-empty for a given event — not all keys
    /// are sent every time, per the wire study.
    Changed {
        zones_added: Vec<Zone>,
        zones_changed: Vec<Zone>,
        zones_removed: Vec<String>,
        zones_seek_changed: Vec<ZoneSeekChange>,
    },
}

#[derive(Deserialize)]
struct SubscribedBody {
    zones: Vec<Zone>,
}

#[derive(Deserialize, Default)]
struct ChangedBody {
    #[serde(default)]
    zones_added: Vec<Zone>,
    #[serde(default)]
    zones_changed: Vec<Zone>,
    #[serde(default)]
    zones_removed: Vec<String>,
    #[serde(default)]
    zones_seek_changed: Vec<ZoneSeekChange>,
}

/// Parses one `CONTINUE` from an open `subscribe_zones` stream into a [`ZoneEvent`]. A `COMPLETE`
/// (the subscription never gets one of its own while active, per the wire study — only a rejected
/// or ended one does) or any name other than `Subscribed`/`Changed` is reported as
/// [`TransportError::UnexpectedResponse`] rather than guessed at.
fn parse_zone_event(msg: MooMessage) -> Result<ZoneEvent, TransportError> {
    if msg.verb != MooVerb::Continue {
        return Err(TransportError::UnexpectedResponse {
            verb: msg.verb,
            name: msg.name,
        });
    }
    let body = match msg.body {
        Some(MooBody::Json(value)) => value,
        Some(MooBody::Binary { .. }) => {
            return Err(TransportError::NonJsonBody { name: msg.name });
        }
        None => return Err(TransportError::MissingBody { name: msg.name }),
    };

    match msg.name.as_str() {
        "Subscribed" => {
            let parsed: SubscribedBody =
                serde_json::from_value(body).map_err(|source| TransportError::MalformedBody {
                    name: msg.name.clone(),
                    source,
                })?;
            Ok(ZoneEvent::Subscribed {
                zones: parsed.zones,
            })
        }
        "Changed" => {
            let parsed: ChangedBody =
                serde_json::from_value(body).map_err(|source| TransportError::MalformedBody {
                    name: msg.name.clone(),
                    source,
                })?;
            Ok(ZoneEvent::Changed {
                zones_added: parsed.zones_added,
                zones_changed: parsed.zones_changed,
                zones_removed: parsed.zones_removed,
                zones_seek_changed: parsed.zones_seek_changed,
            })
        }
        other => Err(TransportError::UnexpectedResponse {
            verb: msg.verb,
            name: other.to_string(),
        }),
    }
}

/// Sends `subscribe_zones` over `requests` and returns a handle to its event stream. Available as
/// soon as the registry handshake completes (see `ConnectionRequests::send_request`'s own doc
/// comment) — a caller wanting to wait for `Paired` first enforces that itself.
pub fn subscribe_zones(requests: &ConnectionRequests) -> Result<ZoneSubscription, TransportError> {
    let body = serde_json::json!({ "subscription_key": SUBSCRIPTION_KEY });
    let stream = requests.send_request(SUBSCRIBE_ZONES, Some(body))?;
    Ok(ZoneSubscription {
        stream,
        done: false,
    })
}

/// An open `subscribe_zones` stream. Dropping it is the only way to end interest early this phase
/// — no `unsubscribe_zones` call exists yet, per IMPL_TRANSPORT.md Phase 2's design notes;
/// `connection`'s own dispatch-table cleanup already tolerates that lazily.
#[derive(Debug)]
pub struct ZoneSubscription {
    stream: MooResponseStream,
    /// Set once `recv` has yielded an `Err` — a malformed or unrecognized response ends the
    /// subscription for good rather than trying to resync, mirroring `moo::transport`'s own
    /// framing-violation handling.
    done: bool,
}

impl ZoneSubscription {
    /// Returns the next zone event, or `None` once the subscription has ended — because the
    /// underlying connection was lost, the Core closed this request, or a previous call already
    /// returned an `Err`. Re-issuing `subscribe_zones` after a reconnect (if wanted) is entirely
    /// up to the caller; this type never does it automatically.
    pub async fn recv(&mut self) -> Option<Result<ZoneEvent, TransportError>> {
        if self.done {
            return None;
        }
        let msg = self.stream.recv().await?;
        let event = parse_zone_event(msg);
        if event.is_err() {
            self.done = true;
        }
        Some(event)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tokio::sync::mpsc;

    use super::*;

    fn continue_msg(name: &str, body: Option<serde_json::Value>) -> MooMessage {
        MooMessage {
            verb: MooVerb::Continue,
            name: name.to_string(),
            request_id: 3,
            headers: HashMap::new(),
            body: body.map(MooBody::Json),
        }
    }

    #[test]
    fn parses_subscribed_with_full_zone_list() {
        let msg = continue_msg(
            "Subscribed",
            Some(serde_json::json!({
                "zones": [{
                    "zone_id": "zone-1",
                    "display_name": "Living Room",
                    "state": "stopped",
                    "is_previous_allowed": false,
                    "is_next_allowed": false,
                    "is_pause_allowed": false,
                    "is_play_allowed": true,
                    "is_seek_allowed": false,
                    "settings": { "loop": "disabled", "shuffle": false, "auto_radio": false },
                }],
            })),
        );

        let event = parse_zone_event(msg).expect("parses");

        match event {
            ZoneEvent::Subscribed { zones } => assert_eq!(zones[0].zone_id, "zone-1"),
            other => panic!("expected Subscribed, got {other:?}"),
        }
    }

    #[test]
    fn parses_changed_with_only_seek_changed_present() {
        let msg = continue_msg(
            "Changed",
            Some(serde_json::json!({
                "zones_seek_changed": [
                    { "zone_id": "zone-1", "seek_position": 42.0, "queue_time_remaining": 100.0 },
                ],
            })),
        );

        let event = parse_zone_event(msg).expect("parses");

        match event {
            ZoneEvent::Changed {
                zones_added,
                zones_changed,
                zones_removed,
                zones_seek_changed,
            } => {
                assert!(zones_added.is_empty());
                assert!(zones_changed.is_empty());
                assert!(zones_removed.is_empty());
                assert_eq!(zones_seek_changed[0].zone_id, "zone-1");
                assert_eq!(zones_seek_changed[0].seek_position, Some(42.0));
            }
            other => panic!("expected Changed, got {other:?}"),
        }
    }

    #[test]
    fn parses_changed_with_only_zones_removed_present() {
        let msg = continue_msg(
            "Changed",
            Some(serde_json::json!({ "zones_removed": ["zone-1"] })),
        );

        let event = parse_zone_event(msg).expect("parses");

        match event {
            ZoneEvent::Changed { zones_removed, .. } => {
                assert_eq!(zones_removed, vec!["zone-1".to_string()])
            }
            other => panic!("expected Changed, got {other:?}"),
        }
    }

    #[test]
    fn rejects_missing_body() {
        let msg = continue_msg("Changed", None);

        assert!(matches!(
            parse_zone_event(msg),
            Err(TransportError::MissingBody { name }) if name == "Changed"
        ));
    }

    #[test]
    fn rejects_malformed_body() {
        let msg = continue_msg(
            "Subscribed",
            Some(serde_json::json!({ "zones": "not a list" })),
        );

        assert!(matches!(
            parse_zone_event(msg),
            Err(TransportError::MalformedBody { name, .. }) if name == "Subscribed"
        ));
    }

    #[test]
    fn rejects_unrecognized_name() {
        let msg = continue_msg("SomethingElse", Some(serde_json::json!({})));

        assert!(matches!(
            parse_zone_event(msg),
            Err(TransportError::UnexpectedResponse { name, .. }) if name == "SomethingElse"
        ));
    }

    #[test]
    fn rejects_a_complete_as_unexpected() {
        let msg = MooMessage {
            verb: MooVerb::Complete,
            name: "InvalidRequest".to_string(),
            request_id: 3,
            headers: HashMap::new(),
            body: None,
        };

        assert!(matches!(
            parse_zone_event(msg),
            Err(TransportError::UnexpectedResponse { verb: MooVerb::Complete, name })
                if name == "InvalidRequest"
        ));
    }

    #[tokio::test]
    async fn zone_subscription_forwards_events_until_the_stream_ends() {
        let (tx, rx) = mpsc::unbounded_channel::<MooMessage>();
        let mut subscription = ZoneSubscription {
            stream: MooResponseStream::new(rx),
            done: false,
        };

        tx.send(continue_msg(
            "Subscribed",
            Some(serde_json::json!({ "zones": [] })),
        ))
        .expect("channel open");
        assert!(matches!(
            subscription.recv().await,
            Some(Ok(ZoneEvent::Subscribed { .. }))
        ));

        drop(tx);
        assert!(
            subscription.recv().await.is_none(),
            "ends once the underlying stream ends, same as a lost connection"
        );
    }

    #[tokio::test]
    async fn zone_subscription_ends_for_good_after_an_error() {
        let (tx, rx) = mpsc::unbounded_channel::<MooMessage>();
        let mut subscription = ZoneSubscription {
            stream: MooResponseStream::new(rx),
            done: false,
        };

        tx.send(continue_msg("Changed", None))
            .expect("channel open");
        assert!(matches!(
            subscription.recv().await,
            Some(Err(TransportError::MissingBody { .. }))
        ));

        // Even though the channel is still open with more messages queued, the subscription
        // doesn't try to resync — it stays ended.
        tx.send(continue_msg("Changed", Some(serde_json::json!({}))))
            .expect("channel open");
        assert!(subscription.recv().await.is_none());
    }
}
