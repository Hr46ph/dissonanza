//! MOO message framing (parsing and encoding), per `docs/protocol/sood-moo.md`.
//!
//! One WebSocket message equals one MOO message — the WS layer already gives framing,
//! so this module never has to resync mid-stream. Used by `moo/transport.rs` to
//! send/receive these over the connection, and by `connection/mod.rs` to dispatch
//! inbound requests to the services this extension provides.

use std::collections::HashMap;

const PROTOCOL: &str = "MOO/1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MooVerb {
    Request,
    Continue,
    Complete,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MooBody {
    Json(serde_json::Value),
    Binary {
        content_type: String,
        bytes: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct MooMessage {
    pub verb: MooVerb,
    pub name: String,
    pub request_id: u32,
    pub headers: HashMap<String, String>,
    pub body: Option<MooBody>,
}

#[derive(Debug, thiserror::Error)]
pub enum MooError {
    #[error("message is missing the blank line separating headers from body")]
    MissingBlankLine,
    #[error("header block is not valid UTF-8")]
    InvalidHeaderUtf8,
    #[error("empty header block: no status line")]
    MissingStatusLine,
    #[error("malformed status line: {0:?}")]
    MalformedStatusLine(String),
    #[error("unknown verb: {0}")]
    UnknownVerb(String),
    #[error("malformed header line: {0:?}")]
    MalformedHeaderLine(String),
    #[error("missing required Request-Id header")]
    MissingRequestId,
    #[error("invalid Request-Id value: {0:?}")]
    InvalidRequestId(String),
    #[error("Content-Type header present without Content-Length")]
    ContentTypeWithoutContentLength,
    #[error("Content-Length header present without Content-Type")]
    ContentLengthWithoutContentType,
    #[error("invalid Content-Length value: {0:?}")]
    InvalidContentLength(String),
    #[error("body length mismatch: Content-Length declared {expected}, got {actual}")]
    BodyLengthMismatch { expected: usize, actual: usize },
    #[error("body present with no Content-Length declared")]
    UnexpectedBody,
    #[error("invalid JSON body: {0}")]
    InvalidJsonBody(#[from] serde_json::Error),
}

impl MooMessage {
    /// Parses a single MOO message. `bytes` must be exactly one message — the caller
    /// (the websocket transport) already provides that framing, one WS message at a
    /// time, so this never has to search for a boundary beyond the header/body blank
    /// line.
    pub fn decode(bytes: &[u8]) -> Result<Self, MooError> {
        let separator = bytes
            .windows(2)
            .position(|w| w == b"\n\n")
            .ok_or(MooError::MissingBlankLine)?;
        let header_text =
            std::str::from_utf8(&bytes[..separator]).map_err(|_| MooError::InvalidHeaderUtf8)?;
        let body_bytes = &bytes[separator + 2..];

        let mut lines = header_text.lines();
        let status_line = lines.next().ok_or(MooError::MissingStatusLine)?;
        let mut status_parts = status_line.splitn(3, ' ');
        let proto = status_parts
            .next()
            .ok_or_else(|| MooError::MalformedStatusLine(status_line.to_string()))?;
        if proto != PROTOCOL {
            return Err(MooError::MalformedStatusLine(status_line.to_string()));
        }
        let verb_token = status_parts
            .next()
            .ok_or_else(|| MooError::MalformedStatusLine(status_line.to_string()))?;
        let name = status_parts
            .next()
            .ok_or_else(|| MooError::MalformedStatusLine(status_line.to_string()))?
            .to_string();

        let verb = match verb_token {
            "REQUEST" => MooVerb::Request,
            "CONTINUE" => MooVerb::Continue,
            "COMPLETE" => MooVerb::Complete,
            other => return Err(MooError::UnknownVerb(other.to_string())),
        };

        let mut headers = HashMap::new();
        for line in lines {
            let (key, value) = line
                .split_once(": ")
                .ok_or_else(|| MooError::MalformedHeaderLine(line.to_string()))?;
            headers.insert(key.to_string(), value.to_string());
        }

        let request_id_str = headers
            .remove("Request-Id")
            .ok_or(MooError::MissingRequestId)?;
        let request_id: u32 = request_id_str
            .parse()
            .map_err(|_| MooError::InvalidRequestId(request_id_str))?;

        let content_length = headers.remove("Content-Length");
        let content_type = headers.remove("Content-Type");

        let body = match (content_length, content_type) {
            (None, None) => {
                if !body_bytes.is_empty() {
                    return Err(MooError::UnexpectedBody);
                }
                None
            }
            (Some(_), None) => return Err(MooError::ContentLengthWithoutContentType),
            (None, Some(_)) => return Err(MooError::ContentTypeWithoutContentLength),
            (Some(len_str), Some(content_type)) => {
                let expected: usize = len_str
                    .parse()
                    .map_err(|_| MooError::InvalidContentLength(len_str.clone()))?;
                if expected != body_bytes.len() {
                    return Err(MooError::BodyLengthMismatch {
                        expected,
                        actual: body_bytes.len(),
                    });
                }
                if content_type == "application/json" {
                    Some(MooBody::Json(serde_json::from_slice(body_bytes)?))
                } else {
                    Some(MooBody::Binary {
                        content_type,
                        bytes: body_bytes.to_vec(),
                    })
                }
            }
        };

        Ok(MooMessage {
            verb,
            name,
            request_id,
            headers,
            body,
        })
    }

    /// Encodes this message to the MOO wire format.
    pub fn encode(&self) -> Vec<u8> {
        let verb_token = match self.verb {
            MooVerb::Request => "REQUEST",
            MooVerb::Continue => "CONTINUE",
            MooVerb::Complete => "COMPLETE",
        };

        let mut out = Vec::new();
        out.extend_from_slice(format!("{PROTOCOL} {verb_token} {}\n", self.name).as_bytes());
        out.extend_from_slice(format!("Request-Id: {}\n", self.request_id).as_bytes());
        for (key, value) in &self.headers {
            out.extend_from_slice(format!("{key}: {value}\n").as_bytes());
        }

        let body_payload = self.body.as_ref().map(|body| match body {
            MooBody::Json(value) => (
                "application/json".to_string(),
                // A `serde_json::Value` built from valid JSON (parsed, or constructed
                // through its own API) can never contain a non-finite float — the only
                // thing that makes serialization fail — so this is infallible in
                // practice.
                serde_json::to_vec(value).expect("serde_json::Value serialization is infallible"),
            ),
            MooBody::Binary {
                content_type,
                bytes,
            } => (content_type.clone(), bytes.clone()),
        });

        if let Some((content_type, bytes)) = &body_payload {
            out.extend_from_slice(format!("Content-Length: {}\n", bytes.len()).as_bytes());
            out.extend_from_slice(format!("Content-Type: {content_type}\n").as_bytes());
        }
        out.push(b'\n');
        if let Some((_, bytes)) = body_payload {
            out.extend_from_slice(&bytes);
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(head: &str, body: &[u8]) -> Vec<u8> {
        let mut bytes = head.as_bytes().to_vec();
        bytes.extend_from_slice(b"\n\n");
        bytes.extend_from_slice(body);
        bytes
    }

    #[test]
    fn parses_request_without_body() {
        let bytes = frame(
            "MOO/1 REQUEST com.roonlabs.registry:1/info\nRequest-Id: 1",
            b"",
        );

        let msg = MooMessage::decode(&bytes).expect("valid message");

        assert_eq!(msg.verb, MooVerb::Request);
        assert_eq!(msg.name, "com.roonlabs.registry:1/info");
        assert_eq!(msg.request_id, 1);
        assert!(msg.headers.is_empty());
        assert_eq!(msg.body, None);
    }

    #[test]
    fn parses_complete_with_json_body() {
        let body = br#"{"core_id":"abc"}"#;
        let bytes = frame(
            &format!(
                "MOO/1 COMPLETE Registered\nRequest-Id: 7\nContent-Length: {}\nContent-Type: application/json",
                body.len()
            ),
            body,
        );

        let msg = MooMessage::decode(&bytes).expect("valid message");

        assert_eq!(msg.verb, MooVerb::Complete);
        assert_eq!(msg.name, "Registered");
        assert_eq!(msg.request_id, 7);
        assert_eq!(
            msg.body,
            Some(MooBody::Json(serde_json::json!({"core_id": "abc"})))
        );
    }

    #[test]
    fn continue_preserves_request_id() {
        let bytes = frame("MOO/1 CONTINUE Changed\nRequest-Id: 42", b"");

        let msg = MooMessage::decode(&bytes).expect("valid message");

        assert_eq!(msg.verb, MooVerb::Continue);
        assert_eq!(msg.request_id, 42);
    }

    #[test]
    fn encode_decode_round_trip() {
        let mut headers = HashMap::new();
        headers.insert("Logging".to_string(), "quiet".to_string());

        let msg = MooMessage {
            verb: MooVerb::Request,
            name: "com.roonlabs.transport:2/subscribe_zones".to_string(),
            request_id: 3,
            headers,
            body: Some(MooBody::Json(serde_json::json!({"foo": "bar"}))),
        };

        let bytes = msg.encode();
        let decoded = MooMessage::decode(&bytes).expect("round-trips");

        assert_eq!(decoded, msg);
    }

    #[test]
    fn rejects_missing_request_id() {
        let bytes = frame("MOO/1 REQUEST com.roonlabs.registry:1/info", b"");

        assert!(matches!(
            MooMessage::decode(&bytes),
            Err(MooError::MissingRequestId)
        ));
    }

    #[test]
    fn rejects_content_type_without_content_length() {
        let bytes = frame(
            "MOO/1 COMPLETE Success\nRequest-Id: 1\nContent-Type: application/json",
            b"",
        );

        assert!(matches!(
            MooMessage::decode(&bytes),
            Err(MooError::ContentTypeWithoutContentLength)
        ));
    }

    #[test]
    fn rejects_content_length_without_content_type() {
        let bytes = frame(
            "MOO/1 COMPLETE Success\nRequest-Id: 1\nContent-Length: 0",
            b"",
        );

        assert!(matches!(
            MooMessage::decode(&bytes),
            Err(MooError::ContentLengthWithoutContentType)
        ));
    }

    #[test]
    fn rejects_body_length_mismatch() {
        let bytes = frame(
            "MOO/1 COMPLETE Success\nRequest-Id: 1\nContent-Length: 100\nContent-Type: application/json",
            b"{}",
        );

        assert!(matches!(
            MooMessage::decode(&bytes),
            Err(MooError::BodyLengthMismatch {
                expected: 100,
                actual: 2
            })
        ));
    }

    #[test]
    fn rejects_unknown_verb() {
        let bytes = frame("MOO/1 FROBNICATE something\nRequest-Id: 1", b"");

        assert!(matches!(
            MooMessage::decode(&bytes),
            Err(MooError::UnknownVerb(verb)) if verb == "FROBNICATE"
        ));
    }

    #[test]
    fn non_json_content_type_kept_as_raw_bytes() {
        let body = b"\x00\x01\x02binary";
        let bytes = frame(
            &format!(
                "MOO/1 COMPLETE Success\nRequest-Id: 1\nContent-Length: {}\nContent-Type: application/octet-stream",
                body.len()
            ),
            body,
        );

        let msg = MooMessage::decode(&bytes).expect("valid message");

        assert_eq!(
            msg.body,
            Some(MooBody::Binary {
                content_type: "application/octet-stream".to_string(),
                bytes: body.to_vec(),
            })
        );
    }

    #[test]
    fn arbitrary_custom_headers_preserved() {
        let bytes = frame(
            "MOO/1 REQUEST com.roonlabs.ping:1/ping\nRequest-Id: 1\nLogging: quiet\nX-Custom: value",
            b"",
        );

        let msg = MooMessage::decode(&bytes).expect("valid message");

        assert_eq!(msg.headers.get("Logging"), Some(&"quiet".to_string()));
        assert_eq!(msg.headers.get("X-Custom"), Some(&"value".to_string()));
    }
}
