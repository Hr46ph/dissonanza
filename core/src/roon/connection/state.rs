//! Connection lifecycle states and the events [`super::Connection::spawn`] emits as it moves
//! between them, per CLAUDE.md §1.

use super::error::ConnectionError;

/// Where a [`super::Connection`] is in its lifecycle toward — and while holding — a pairing with
/// a Roon Core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    /// Running SOOD discovery, no Core found yet.
    Discovering,
    /// A Core was discovered; opening the MOO websocket to it.
    Connecting,
    /// The MOO websocket is open and the registry handshake completed; waiting for the user to
    /// pair this extension in Roon's UI.
    Registering,
    /// The Core sent a `pair` request over the `com.roonlabs.pairing:1` service this extension
    /// provides.
    Paired { core_id: String },
    /// The connection ended: the transport closed, the app-level keepalive went stale with no
    /// activity, a step failed, or shutdown was requested. This step never retries on its own —
    /// reconnecting by returning to `Discovering` is a later step's responsibility, per
    /// CLAUDE.md's mandatory technical choices (never redial a stale address).
    Disconnected,
}

/// Emitted on [`super::Connection::spawn`]'s event channel.
#[derive(Debug)]
pub enum ConnectionEvent {
    /// The connection moved to a new state.
    StateChanged(ConnectionState),
    /// A step failed. Always immediately followed by
    /// `StateChanged(ConnectionState::Disconnected)`.
    Error(ConnectionError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paired_states_compare_by_core_id() {
        assert_eq!(
            ConnectionState::Paired {
                core_id: "core-1".to_string()
            },
            ConnectionState::Paired {
                core_id: "core-1".to_string()
            }
        );
        assert_ne!(
            ConnectionState::Paired {
                core_id: "core-1".to_string()
            },
            ConnectionState::Paired {
                core_id: "core-2".to_string()
            }
        );
    }
}
