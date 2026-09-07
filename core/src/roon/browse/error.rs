//! Errors from `core::roon::browse`'s own requests, parallel to
//! `transport::error::TransportError` — same seven variants, generalized for a service whose
//! `Success` response carries a body to parse rather than an empty one.

use crate::roon::connection::{ConnectionRequestError, MooVerb};

#[derive(Debug, thiserror::Error)]
pub enum BrowseError {
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
    #[error("unexpected {verb:?} {name} response")]
    UnexpectedResponse { verb: MooVerb, name: String },
    #[error("no response to {name}")]
    NoResponse { name: String },
    #[error("{name} command failed")]
    CommandFailed { name: String },
}
