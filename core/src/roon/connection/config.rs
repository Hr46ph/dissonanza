//! Extension identity declared to a Roon Core during the `com.roonlabs.registry:1/register`
//! handshake step, per `docs/protocol/sood-moo.md`.
//!
//! Not wired into a connection state machine yet — `connection/mod.rs` (a later step) is what
//! actually takes this as `Connection::spawn`'s input. Until then this module's items are
//! unused outside their own tests.
#![allow(dead_code)]

/// Identifies this extension to a Roon Core. `website` is the only field the wire protocol
/// allows to be omitted.
#[derive(Debug, Clone)]
pub(crate) struct ConnectionConfig {
    pub extension_id: String,
    pub display_name: String,
    pub display_version: String,
    pub publisher: String,
    pub email: String,
    pub website: Option<String>,
}
