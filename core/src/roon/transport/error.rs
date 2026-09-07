//! Errors from `core::roon::transport`'s own requests, parallel to
//! `connection::error::ConnectionError`. Distinct from (and unrelated to)
//! `connection::TransportError`, which is the MOO *websocket* transport's error — see
//! IMPL_TRANSPORT.md Phase 2's design notes for why the name is shared.

use crate::roon::connection::{ConnectionRequestError, MooVerb};

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("send request: {0}")]
    Request(#[from] ConnectionRequestError),
    #[error("{name} response had no body")]
    MissingBody { name: String },
    #[error("{name} response body was binary, not JSON")]
    NonJsonBody { name: String },
    #[error("malformed {name} body: {source}")]
    MalformedBody {
        name: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("unexpected {verb:?} {name} for a zone subscription")]
    UnexpectedResponse { verb: MooVerb, name: String },
}
