//! `browse`/`load`, per docs/protocol/browse.md's "Request/response verbs" section — both
//! one-shot, single-`COMPLETE` RPCs, no subscription. Built entirely on
//! `connection::ConnectionRequests`, same as `transport::control` — never touches SOOD discovery,
//! pairing, or reconnect itself.
//!
//! Unlike `transport:2`'s control verbs, a `Success` response here carries a body (the browse
//! result or the loaded page) rather than being empty — `parse_response`/`await_response` are
//! generic over the response type accordingly.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::roon::connection::{
    ConnectionRequests, MooBody, MooMessage, MooResponseStream, MooVerb,
};

use super::error::BrowseError;
use super::model::{Item, List};

const BROWSE: &str = "com.roonlabs.browse:1/browse";
const LOAD: &str = "com.roonlabs.browse:1/load";

/// `hierarchy`, per docs/protocol/browse.md — identifies which navigation tree is being browsed.
/// Required on every `browse`/`load` call, unlike `TheAppgineer/rust-roon-api`'s port, which
/// hardcodes `"browse"` internally and can't reach the other seven.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Hierarchy {
    Browse,
    Playlists,
    Settings,
    InternetRadio,
    Albums,
    Artists,
    Genres,
    Composers,
    Search,
}

/// Options for a `browse` call. `hierarchy` is required; everything else defaults to omitted
/// (the Core's own defaults apply) until set. No `Default` impl — there's no sensible default
/// `hierarchy` to pick, so `new` takes it explicitly rather than risking a caller silently
/// omitting a required field.
#[derive(Debug, Clone, Serialize)]
pub struct BrowseOptions {
    pub hierarchy: Hierarchy,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multi_session_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone_or_output_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pop_all: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pop_levels: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_list: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub set_display_offset: Option<u32>,
}

impl BrowseOptions {
    pub fn new(hierarchy: Hierarchy) -> Self {
        Self {
            hierarchy,
            multi_session_key: None,
            item_key: None,
            input: None,
            zone_or_output_id: None,
            pop_all: None,
            pop_levels: None,
            refresh_list: None,
            set_display_offset: None,
        }
    }
}

/// Options for a `load` call. Same `hierarchy`/`Default` reasoning as [`BrowseOptions`].
#[derive(Debug, Clone, Serialize)]
pub struct LoadOptions {
    pub hierarchy: Hierarchy,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multi_session_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub set_display_offset: Option<u32>,
}

impl LoadOptions {
    pub fn new(hierarchy: Hierarchy) -> Self {
        Self {
            hierarchy,
            multi_session_key: None,
            level: None,
            offset: None,
            count: None,
            set_display_offset: None,
        }
    }
}

/// `action`, per docs/protocol/browse.md — what the client should do as a result of a `browse`
/// call. No forward-compatibility note in the JSDoc for this one (unlike `hint`), so an
/// unrecognized value is a [`BrowseError::MalformedBody`] rather than a silent catch-all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowseAction {
    Message,
    None,
    List,
    ReplaceItem,
    RemoveItem,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BrowseResult {
    pub action: BrowseAction,
    #[serde(default)]
    pub list: Option<List>,
    #[serde(default)]
    pub item: Option<Item>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub is_error: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct LoadResult {
    #[serde(default)]
    pub items: Vec<Item>,
    pub offset: u64,
    pub list: List,
}

/// Sends `browse` and waits for its `COMPLETE`, parsing a `Success` body into a [`BrowseResult`].
/// Available as soon as the registry handshake completes, same as `transport::control`'s verbs —
/// not gated on `Paired`.
pub async fn browse(
    requests: &ConnectionRequests,
    opts: &BrowseOptions,
) -> Result<BrowseResult, BrowseError> {
    let body = serde_json::to_value(opts)
        .expect("BrowseOptions serializes to JSON infallibly (no non-string keys or NaN floats)");
    let mut stream = requests.send_request(BROWSE, Some(body))?;
    await_response(BROWSE, &mut stream).await
}

/// Sends `load` and waits for its `COMPLETE`, parsing a `Success` body into a [`LoadResult`].
pub async fn load(
    requests: &ConnectionRequests,
    opts: &LoadOptions,
) -> Result<LoadResult, BrowseError> {
    let body = serde_json::to_value(opts)
        .expect("LoadOptions serializes to JSON infallibly (no non-string keys or NaN floats)");
    let mut stream = requests.send_request(LOAD, Some(body))?;
    await_response(LOAD, &mut stream).await
}

/// Waits for the single `COMPLETE` `browse`/`load` reply with, per docs/protocol/browse.md ("a
/// single `COMPLETE` (`Success`, or an error name, no `CONTINUE`s)"). `request_name` only names
/// the request in `NoResponse` if the stream ends before anything arrives.
async fn await_response<T: DeserializeOwned>(
    request_name: &str,
    stream: &mut MooResponseStream,
) -> Result<T, BrowseError> {
    let msg = stream.recv().await.ok_or_else(|| BrowseError::NoResponse {
        name: request_name.to_string(),
    })?;
    parse_response(msg)
}

/// Pure parsing half of [`await_response`], split out so it can be unit-tested directly against
/// fabricated [`MooMessage`]s, mirroring `transport::control`'s `parse_command_response`.
fn parse_response<T: DeserializeOwned>(msg: MooMessage) -> Result<T, BrowseError> {
    if msg.verb != MooVerb::Complete {
        return Err(BrowseError::UnexpectedResponse {
            verb: msg.verb,
            name: msg.name,
        });
    }
    if msg.name != "Success" {
        return Err(BrowseError::CommandFailed { name: msg.name });
    }
    let body = match msg.body {
        Some(MooBody::Json(value)) => value,
        Some(MooBody::Binary { .. }) => {
            return Err(BrowseError::NonJsonBody { name: msg.name });
        }
        None => return Err(BrowseError::MissingBody { name: msg.name }),
    };
    serde_json::from_value(body).map_err(|source| BrowseError::MalformedBody {
        name: msg.name.clone(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tokio::sync::mpsc;

    use super::*;

    fn complete_msg(name: &str, body: Option<serde_json::Value>) -> MooMessage {
        MooMessage {
            verb: MooVerb::Complete,
            name: name.to_string(),
            request_id: 3,
            headers: HashMap::new(),
            body: body.map(MooBody::Json),
        }
    }

    #[test]
    fn parses_a_success_list_action_into_browse_result() {
        let msg = complete_msg(
            "Success",
            Some(serde_json::json!({
                "action": "list",
                "list": { "title": "Albums", "count": 5, "level": 1 },
            })),
        );

        let result: BrowseResult = parse_response(msg).expect("parses");

        assert_eq!(result.action, BrowseAction::List);
        assert_eq!(result.list.expect("list present").title, "Albums");
    }

    #[test]
    fn parses_a_success_message_action_with_is_error() {
        let msg = complete_msg(
            "Success",
            Some(serde_json::json!({
                "action": "message",
                "message": "No results found",
                "is_error": false,
            })),
        );

        let result: BrowseResult = parse_response(msg).expect("parses");

        assert_eq!(result.action, BrowseAction::Message);
        assert_eq!(result.message.as_deref(), Some("No results found"));
        assert_eq!(result.is_error, Some(false));
    }

    #[test]
    fn parses_a_load_result() {
        let msg = complete_msg(
            "Success",
            Some(serde_json::json!({
                "items": [{ "title": "Track 1" }],
                "offset": 0,
                "list": { "title": "Album", "count": 1, "level": 2 },
            })),
        );

        let result: LoadResult = parse_response(msg).expect("parses");

        assert_eq!(result.items[0].title, "Track 1");
        assert_eq!(result.list.title, "Album");
    }

    #[test]
    fn a_non_success_complete_is_command_failed() {
        let msg = complete_msg("ZoneNotFound", None);

        assert!(matches!(
            parse_response::<BrowseResult>(msg),
            Err(BrowseError::CommandFailed { name }) if name == "ZoneNotFound"
        ));
    }

    #[test]
    fn a_success_with_no_body_is_missing_body() {
        let msg = complete_msg("Success", None);

        assert!(matches!(
            parse_response::<BrowseResult>(msg),
            Err(BrowseError::MissingBody { name }) if name == "Success"
        ));
    }

    #[test]
    fn a_continue_is_unexpected() {
        let msg = MooMessage {
            verb: MooVerb::Continue,
            ..complete_msg("Success", None)
        };

        assert!(matches!(
            parse_response::<BrowseResult>(msg),
            Err(BrowseError::UnexpectedResponse { verb: MooVerb::Continue, name })
                if name == "Success"
        ));
    }

    #[tokio::test]
    async fn await_response_forwards_the_first_message() {
        let (tx, rx) = mpsc::unbounded_channel::<MooMessage>();
        let mut stream = MooResponseStream::new(rx);
        tx.send(complete_msg(
            "Success",
            Some(serde_json::json!({ "action": "none" })),
        ))
        .expect("channel open");

        let result: BrowseResult = await_response(BROWSE, &mut stream).await.expect("parses");
        assert_eq!(result.action, BrowseAction::None);
    }

    #[tokio::test]
    async fn await_response_reports_no_response_when_the_stream_ends_empty() {
        let (tx, rx) = mpsc::unbounded_channel::<MooMessage>();
        let mut stream = MooResponseStream::new(rx);
        drop(tx);

        assert!(matches!(
            await_response::<BrowseResult>(BROWSE, &mut stream).await,
            Err(BrowseError::NoResponse { name }) if name == BROWSE
        ));
    }

    #[test]
    fn hierarchy_serializes_to_wire_names() {
        assert_eq!(
            serde_json::to_value(Hierarchy::InternetRadio).unwrap(),
            serde_json::json!("internet_radio")
        );
        assert_eq!(
            serde_json::to_value(Hierarchy::Browse).unwrap(),
            serde_json::json!("browse")
        );
    }

    #[test]
    fn browse_options_omits_unset_fields() {
        let opts = BrowseOptions::new(Hierarchy::Search);

        assert_eq!(
            serde_json::to_value(&opts).unwrap(),
            serde_json::json!({ "hierarchy": "search" })
        );
    }

    #[test]
    fn browse_options_includes_fields_once_set() {
        let mut opts = BrowseOptions::new(Hierarchy::Browse);
        opts.pop_all = Some(true);
        opts.item_key = Some("item-1".to_string());

        assert_eq!(
            serde_json::to_value(&opts).unwrap(),
            serde_json::json!({ "hierarchy": "browse", "item_key": "item-1", "pop_all": true })
        );
    }

    #[test]
    fn load_options_omits_unset_fields() {
        let opts = LoadOptions::new(Hierarchy::Albums);

        assert_eq!(
            serde_json::to_value(&opts).unwrap(),
            serde_json::json!({ "hierarchy": "albums" })
        );
    }
}
