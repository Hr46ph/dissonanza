//! Bridges `core::roon::connection`'s `tokio`-async, channel-based API onto Slint's own blocking
//! UI event loop (IMPL_UI_SHELL.md Phase 1's open architectural question). Owns a background OS
//! thread running a dedicated `tokio` runtime that drives `Connection::spawn`; forwards
//! `ConnectionEvent`s to the UI thread via `slint::invoke_from_event_loop`. `.slint` files stay
//! pure view — no `core` types or async logic are reachable from them directly.

use std::thread;

use dissonanza_core::roon::connection::{
    Connection, ConnectionConfig, ConnectionEvent, ConnectionState,
};
use slint::Weak;

use crate::AppWindow;

/// Extension identity declared to a Roon Core during registration — this is what a user sees in
/// Roon's Settings > Extensions when pairing. `email`/`website` are placeholders (an `.invalid`
/// email per RFC 2606, a guessed repo URL) pending the user's real contact details — flagged here
/// rather than silently shipped, trivial to correct in one place.
fn connection_config() -> ConnectionConfig {
    ConnectionConfig {
        extension_id: "io.github.hr46ph.dissonanza".to_string(),
        display_name: "Dissonanza".to_string(),
        display_version: env!("CARGO_PKG_VERSION").to_string(),
        publisher: "Hr46ph".to_string(),
        email: "dev@dissonanza.invalid".to_string(),
        website: Some("https://github.com/hr46ph/dissonanza".to_string()),
    }
}

/// Spawns the background thread that drives `core::roon::connection` and forwards its events to
/// `ui`. Fire-and-forget by design: Phase 1's deliverable is proving the bridge works end to end,
/// not a clean shutdown path — window-close handling is left to a later phase (see
/// IMPL_UI_SHELL.md).
pub fn spawn(ui: Weak<AppWindow>) {
    thread::spawn(move || run(ui));
}

fn run(ui: Weak<AppWindow>) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            set_status(&ui, format!("failed to start background runtime: {err}"));
            return;
        }
    };
    runtime.block_on(drive_connection(ui));
}

async fn drive_connection(ui: Weak<AppWindow>) {
    // `_handle`/`_requests` are unused this phase (no shutdown path, no other module sending
    // requests yet) but must stay alive — dropping `_handle` wouldn't stop the connection (per
    // its own doc comment) but dropping `_requests` would close the only way future phases reach
    // it, so both are kept bound for `drive_connection`'s whole lifetime rather than discarded.
    let (_handle, _requests, mut events) = Connection::spawn(connection_config());
    while let Some(event) = events.recv().await {
        match event {
            ConnectionEvent::StateChanged(state) => set_status(&ui, describe_state(&state)),
            ConnectionEvent::Error(err) => set_status(&ui, format!("error: {err}")),
        }
    }
}

fn describe_state(state: &ConnectionState) -> String {
    match state {
        ConnectionState::Discovering => "discovering".to_string(),
        ConnectionState::Connecting => "connecting".to_string(),
        ConnectionState::Registering => "registering".to_string(),
        ConnectionState::Paired { core_id } => format!("paired ({core_id})"),
        ConnectionState::Disconnected => "disconnected".to_string(),
    }
}

fn set_status(ui: &Weak<AppWindow>, status: String) {
    let ui = ui.clone();
    // Ignoring the `Result`: this only errors once the event loop has already ended (window
    // closed), at which point there's nothing left to update.
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(window) = ui.upgrade() {
            window.set_connectionStatus(status.into());
        }
    });
}
