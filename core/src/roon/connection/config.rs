//! Extension identity declared to a Roon Core during the `com.roonlabs.registry:1/register`
//! handshake step, per `docs/protocol/sood-moo.md`. `Connection::spawn`'s input.

/// Identifies this extension to a Roon Core. `website` is the only field the wire protocol
/// allows to be omitted.
#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    pub extension_id: String,
    pub display_name: String,
    pub display_version: String,
    pub publisher: String,
    pub email: String,
    pub website: Option<String>,
}
