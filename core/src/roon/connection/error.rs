//! Errors from [`super::Connection`]'s discover → connect → register pipeline.

use super::moo::handshake::HandshakeError;
use super::moo::transport::TransportError;
use super::sood::discovery::DiscoveryError;

#[derive(Debug, thiserror::Error)]
pub enum ConnectionError {
    #[error("SOOD discovery failed: {0}")]
    Discovery(#[from] DiscoveryError),
    #[error("MOO transport failed: {0}")]
    Transport(#[from] TransportError),
    #[error("registry handshake failed: {0}")]
    Handshake(#[from] HandshakeError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_a_handshake_error_with_context() {
        let err: ConnectionError = HandshakeError::MissingBody.into();
        assert!(matches!(err, ConnectionError::Handshake(_)));
        assert_eq!(
            err.to_string(),
            "registry handshake failed: Registered response had no body"
        );
    }
}
