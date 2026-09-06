//! SOOD packet parsing and encoding (TLV wire format), per
//! `docs/protocol/sood-moo.md`.
//!
//! Not wired into a socket yet — `sood/discovery.rs` (added in a later step) is what
//! actually sends/receives these over UDP multicast. Until then this module is inert
//! scaffolding, so its items are unused outside their own tests.
#![allow(dead_code)]

use std::collections::HashMap;
use std::net::SocketAddr;

const MAGIC: &[u8; 4] = b"SOOD";
const VERSION: u8 = 0x02;
const NULL_VALUE_LEN: u16 = 0xFFFF;
const EMPTY_VALUE_LEN: u16 = 0x0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoodMessageType {
    Query,
    Response,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoodMessage {
    pub from: SocketAddr,
    pub msg_type: SoodMessageType,
    pub props: HashMap<String, Option<String>>,
}

#[derive(Debug, thiserror::Error)]
pub enum SoodError {
    #[error("packet too short: expected at least {expected} bytes, got {actual}")]
    TooShort { expected: usize, actual: usize },
    #[error("bad magic bytes: expected \"SOOD\"")]
    BadMagic,
    #[error("unsupported protocol version: {0:#04x}")]
    UnsupportedVersion(u8),
    #[error("unknown message type byte: {0:#04x}")]
    UnknownType(u8),
    #[error("zero-length property name")]
    EmptyName,
    #[error("truncated property name")]
    TruncatedName,
    #[error("truncated property value length")]
    TruncatedValueLen,
    #[error("truncated property value")]
    TruncatedValue,
    #[error("property name is not valid UTF-8")]
    InvalidNameUtf8,
    #[error("property value is not valid UTF-8")]
    InvalidValueUtf8,
}

impl SoodMessage {
    /// Parses a single SOOD UDP datagram. `from` is the socket source address the caller
    /// received it from — it isn't part of the wire format itself.
    pub fn decode(from: SocketAddr, bytes: &[u8]) -> Result<Self, SoodError> {
        if bytes.len() < 6 {
            return Err(SoodError::TooShort {
                expected: 6,
                actual: bytes.len(),
            });
        }
        if &bytes[0..4] != MAGIC {
            return Err(SoodError::BadMagic);
        }
        let version = bytes[4];
        if version != VERSION {
            return Err(SoodError::UnsupportedVersion(version));
        }
        let msg_type = match bytes[5] {
            b'Q' => SoodMessageType::Query,
            b'R' => SoodMessageType::Response,
            other => return Err(SoodError::UnknownType(other)),
        };

        let mut props = HashMap::new();
        let mut cursor = 6;
        while cursor < bytes.len() {
            let name_len = bytes[cursor] as usize;
            cursor += 1;
            if name_len == 0 {
                return Err(SoodError::EmptyName);
            }
            if cursor + name_len > bytes.len() {
                return Err(SoodError::TruncatedName);
            }
            let name = std::str::from_utf8(&bytes[cursor..cursor + name_len])
                .map_err(|_| SoodError::InvalidNameUtf8)?
                .to_string();
            cursor += name_len;

            if cursor + 2 > bytes.len() {
                return Err(SoodError::TruncatedValueLen);
            }
            let value_len = u16::from_be_bytes([bytes[cursor], bytes[cursor + 1]]);
            cursor += 2;

            let value = match value_len {
                NULL_VALUE_LEN => None,
                EMPTY_VALUE_LEN => Some(String::new()),
                n => {
                    let n = n as usize;
                    if cursor + n > bytes.len() {
                        return Err(SoodError::TruncatedValue);
                    }
                    let s = std::str::from_utf8(&bytes[cursor..cursor + n])
                        .map_err(|_| SoodError::InvalidValueUtf8)?
                        .to_string();
                    cursor += n;
                    Some(s)
                }
            };
            props.insert(name, value);
        }

        Ok(SoodMessage {
            from,
            msg_type,
            props,
        })
    }

    /// Encodes a message body (type + properties) to the SOOD wire format. There's no
    /// `from` to encode — that only exists for a datagram already received.
    ///
    /// Property names/values here are always short, fixed protocol constants we control
    /// (e.g. `query_service_id`), so the length-prefix bounds are a `debug_assert!`
    /// invariant rather than a runtime error path.
    pub fn encode(msg_type: SoodMessageType, props: &HashMap<String, Option<String>>) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(MAGIC);
        buf.push(VERSION);
        buf.push(match msg_type {
            SoodMessageType::Query => b'Q',
            SoodMessageType::Response => b'R',
        });

        for (name, value) in props {
            debug_assert!(
                !name.is_empty() && name.len() <= u8::MAX as usize,
                "SOOD property name must be 1..=255 bytes"
            );
            buf.push(name.len() as u8);
            buf.extend_from_slice(name.as_bytes());

            match value {
                None => buf.extend_from_slice(&NULL_VALUE_LEN.to_be_bytes()),
                Some(v) if v.is_empty() => buf.extend_from_slice(&EMPTY_VALUE_LEN.to_be_bytes()),
                Some(v) => {
                    debug_assert!(
                        v.len() < NULL_VALUE_LEN as usize,
                        "SOOD property value must be < 65535 bytes"
                    );
                    buf.extend_from_slice(&(v.len() as u16).to_be_bytes());
                    buf.extend_from_slice(v.as_bytes());
                }
            }
        }

        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn dummy_addr() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9003)
    }

    fn tlv(name: &str, value_bytes: &[u8]) -> Vec<u8> {
        let mut out = vec![name.len() as u8];
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(value_bytes);
        out
    }

    #[test]
    fn decodes_query_with_null_and_empty_values() {
        let mut bytes = b"SOOD\x02Q".to_vec();
        bytes.extend(tlv("query_service_id", &[0xFF, 0xFF])); // null sentinel
        bytes.extend(tlv("note", &[0x00, 0x00])); // empty string

        let msg = SoodMessage::decode(dummy_addr(), &bytes).expect("valid packet");

        assert_eq!(msg.msg_type, SoodMessageType::Query);
        assert_eq!(msg.props.get("query_service_id"), Some(&None));
        assert_eq!(msg.props.get("note"), Some(&Some(String::new())));
    }

    #[test]
    fn decodes_response_with_valued_property() {
        let mut bytes = b"SOOD\x02R".to_vec();
        let value = b"1234";
        let mut entry = tlv("http_port", &(value.len() as u16).to_be_bytes());
        entry.extend_from_slice(value);
        bytes.extend(entry);

        let msg = SoodMessage::decode(dummy_addr(), &bytes).expect("valid packet");

        assert_eq!(msg.msg_type, SoodMessageType::Response);
        assert_eq!(msg.props.get("http_port"), Some(&Some("1234".to_string())));
    }

    #[test]
    fn retains_unknown_properties_without_schema_changes() {
        let mut bytes = b"SOOD\x02R".to_vec();
        let value = b"whatever";
        let mut entry = tlv("some_future_prop", &(value.len() as u16).to_be_bytes());
        entry.extend_from_slice(value);
        bytes.extend(entry);

        let msg = SoodMessage::decode(dummy_addr(), &bytes).expect("valid packet");

        assert_eq!(
            msg.props.get("some_future_prop"),
            Some(&Some("whatever".to_string()))
        );
    }

    #[test]
    fn rejects_bad_magic() {
        let bytes = b"XXXX\x02Q".to_vec();
        assert!(matches!(
            SoodMessage::decode(dummy_addr(), &bytes),
            Err(SoodError::BadMagic)
        ));
    }

    #[test]
    fn rejects_unsupported_version() {
        let bytes = b"SOOD\x01Q".to_vec();
        assert!(matches!(
            SoodMessage::decode(dummy_addr(), &bytes),
            Err(SoodError::UnsupportedVersion(0x01))
        ));
    }

    #[test]
    fn rejects_unknown_type_byte() {
        let bytes = b"SOOD\x02Z".to_vec();
        assert!(matches!(
            SoodMessage::decode(dummy_addr(), &bytes),
            Err(SoodError::UnknownType(b'Z'))
        ));
    }

    #[test]
    fn rejects_truncated_name() {
        let mut bytes = b"SOOD\x02Q".to_vec();
        bytes.push(10); // claims a 10-byte name
        bytes.extend_from_slice(b"short"); // only 5 bytes follow

        assert!(matches!(
            SoodMessage::decode(dummy_addr(), &bytes),
            Err(SoodError::TruncatedName)
        ));
    }

    #[test]
    fn rejects_truncated_value() {
        let mut bytes = b"SOOD\x02Q".to_vec();
        bytes.push(4);
        bytes.extend_from_slice(b"name");
        bytes.extend_from_slice(&100u16.to_be_bytes()); // claims 100 value bytes
        bytes.extend_from_slice(b"only a few");

        assert!(matches!(
            SoodMessage::decode(dummy_addr(), &bytes),
            Err(SoodError::TruncatedValue)
        ));
    }

    #[test]
    fn encode_decode_round_trip() {
        let mut props = HashMap::new();
        props.insert("query_service_id".to_string(), None);
        props.insert("note".to_string(), Some(String::new()));
        props.insert("unique_id".to_string(), Some("core-123".to_string()));

        let bytes = SoodMessage::encode(SoodMessageType::Query, &props);
        let decoded = SoodMessage::decode(dummy_addr(), &bytes).expect("round-trips");

        assert_eq!(decoded.msg_type, SoodMessageType::Query);
        assert_eq!(decoded.props, props);
    }
}
