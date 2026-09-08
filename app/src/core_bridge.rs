//! Bridges `core::roon::connection`'s `tokio`-async, channel-based API onto Slint's own blocking
//! UI event loop (IMPL_UI_SHELL.md Phase 1's open architectural question). Owns a background OS
//! thread running a dedicated `tokio` runtime that drives `Connection::spawn`; forwards
//! `ConnectionEvent`s to the UI thread via `slint::invoke_from_event_loop`. `.slint` files stay
//! pure view — no `core` types or async logic are reachable from them directly.

use std::thread;

use dissonanza_core::roon::connection::{
    Connection, ConnectionConfig, ConnectionEvent, ConnectionState,
};
use dissonanza_core::roon::transport::{self, TransportError, Zone, ZoneEvent, ZoneSubscription};
use slint::{ModelRc, VecModel, Weak};

use crate::{AppWindow, ZoneInfo};

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
    // `_handle` is unused this phase (no shutdown path yet) but must stay alive — dropping it
    // wouldn't stop the connection (per its own doc comment) but it's kept bound for
    // `drive_connection`'s whole lifetime rather than discarded, matching `requests`'s own
    // lifetime need (Phase 2 now sends `subscribe_zones` through it on every `Paired`).
    let (_handle, requests, mut events) = Connection::spawn(connection_config());
    let mut zones: Vec<Zone> = Vec::new();
    let mut subscription: Option<ZoneSubscription> = None;

    loop {
        tokio::select! {
            event = events.recv() => {
                let Some(event) = event else { break };
                match event {
                    ConnectionEvent::StateChanged(state) => {
                        set_status(&ui, describe_state(&state));
                        subscription = if matches!(state, ConnectionState::Paired { .. }) {
                            match transport::subscribe_zones(&requests) {
                                Ok(sub) => Some(sub),
                                Err(err) => {
                                    set_status(&ui, format!("failed to subscribe to zones: {err}"));
                                    None
                                }
                            }
                        } else {
                            None
                        };
                    }
                    ConnectionEvent::Error(err) => set_status(&ui, format!("error: {err}")),
                }
            }
            zone_event = recv_zone_event(&mut subscription) => {
                match zone_event {
                    Some(Ok(event)) => {
                        apply_zone_event(&mut zones, event);
                        set_zones(&ui, to_zone_infos(&zones));
                    }
                    Some(Err(err)) => {
                        set_status(&ui, format!("zone subscription error: {err}"));
                        subscription = None;
                    }
                    None => subscription = None,
                }
            }
        }
    }
}

/// Awaits the next event from `subscription` if one is open, or never resolves if it's `None` —
/// lets `tokio::select!` treat "no active zone subscription yet" as a branch that simply never
/// fires, rather than needing a separate `select!` arm shape for each case.
async fn recv_zone_event(
    subscription: &mut Option<ZoneSubscription>,
) -> Option<Result<ZoneEvent, TransportError>> {
    match subscription {
        Some(sub) => sub.recv().await,
        None => std::future::pending().await,
    }
}

/// Applies one `ZoneEvent` to the in-memory zone list: `Subscribed` replaces it wholesale,
/// `Changed` applies `zones_added`/`zones_changed`/`zones_removed` by `zone_id`.
/// `zones_seek_changed` is ignored this phase — seek position isn't displayed until Phase 3's
/// transport bar, per IMPL_UI_SHELL.md's Phase 2 design notes.
fn apply_zone_event(zones: &mut Vec<Zone>, event: ZoneEvent) {
    match event {
        ZoneEvent::Subscribed { zones: subscribed } => *zones = subscribed,
        ZoneEvent::Changed {
            zones_added,
            zones_changed,
            zones_removed,
            zones_seek_changed: _,
        } => {
            zones.retain(|zone| !zones_removed.contains(&zone.zone_id));
            for changed in zones_changed {
                match zones
                    .iter_mut()
                    .find(|zone| zone.zone_id == changed.zone_id)
                {
                    Some(existing) => *existing = changed,
                    None => zones.push(changed),
                }
            }
            zones.extend(zones_added);
        }
    }
}

fn to_zone_infos(zones: &[Zone]) -> Vec<ZoneInfo> {
    zones
        .iter()
        .map(|zone| ZoneInfo {
            zoneId: zone.zone_id.clone().into(),
            displayName: zone.display_name.clone().into(),
            state: describe_zone_state(zone.state).into(),
        })
        .collect()
}

fn describe_zone_state(state: transport::ZoneState) -> &'static str {
    match state {
        transport::ZoneState::Playing => "playing",
        transport::ZoneState::Paused => "paused",
        transport::ZoneState::Loading => "loading",
        transport::ZoneState::Stopped => "stopped",
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

fn set_zones(ui: &Weak<AppWindow>, zones: Vec<ZoneInfo>) {
    let ui = ui.clone();
    // Same reasoning as `set_status` for ignoring the `Result`.
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(window) = ui.upgrade() {
            window.set_zones(ModelRc::new(VecModel::from(zones)));
        }
    });
}

#[cfg(test)]
mod tests {
    use dissonanza_core::roon::transport::{LoopMode, ZoneSeekChange, ZoneSettings, ZoneState};

    use super::*;

    fn zone(id: &str, name: &str, state: ZoneState) -> Zone {
        Zone {
            zone_id: id.to_string(),
            display_name: name.to_string(),
            outputs: Vec::new(),
            state,
            seek_position: None,
            is_previous_allowed: false,
            is_next_allowed: false,
            is_pause_allowed: false,
            is_play_allowed: true,
            is_seek_allowed: false,
            queue_items_remaining: None,
            queue_time_remaining: None,
            settings: ZoneSettings {
                loop_mode: LoopMode::Disabled,
                shuffle: false,
                auto_radio: false,
            },
            now_playing: None,
        }
    }

    #[test]
    fn subscribed_replaces_the_whole_list() {
        let mut zones = vec![zone("stale", "Stale Zone", ZoneState::Stopped)];

        apply_zone_event(
            &mut zones,
            ZoneEvent::Subscribed {
                zones: vec![zone("zone-1", "Living Room", ZoneState::Playing)],
            },
        );

        assert_eq!(zones.len(), 1);
        assert_eq!(zones[0].zone_id, "zone-1");
    }

    #[test]
    fn changed_applies_added_changed_and_removed_by_zone_id() {
        let mut zones = vec![
            zone("zone-1", "Living Room", ZoneState::Stopped),
            zone("zone-2", "Kitchen", ZoneState::Stopped),
        ];

        apply_zone_event(
            &mut zones,
            ZoneEvent::Changed {
                zones_added: vec![zone("zone-3", "Office", ZoneState::Stopped)],
                zones_changed: vec![zone("zone-1", "Living Room", ZoneState::Playing)],
                zones_removed: vec!["zone-2".to_string()],
                zones_seek_changed: Vec::new(),
            },
        );

        assert_eq!(zones.len(), 2);
        assert_eq!(zones[0].zone_id, "zone-1");
        assert_eq!(zones[0].state, ZoneState::Playing);
        assert_eq!(zones[1].zone_id, "zone-3");
    }

    #[test]
    fn changed_ignores_seek_changed_entries() {
        let mut zones = vec![zone("zone-1", "Living Room", ZoneState::Playing)];

        apply_zone_event(
            &mut zones,
            ZoneEvent::Changed {
                zones_added: Vec::new(),
                zones_changed: Vec::new(),
                zones_removed: Vec::new(),
                zones_seek_changed: vec![ZoneSeekChange {
                    zone_id: "zone-1".to_string(),
                    seek_position: Some(42.0),
                    queue_time_remaining: 100.0,
                }],
            },
        );

        assert_eq!(zones.len(), 1);
        assert_eq!(zones[0].zone_id, "zone-1");
    }

    #[test]
    fn maps_zones_to_zone_infos() {
        let zones = vec![
            zone("zone-1", "Living Room", ZoneState::Playing),
            zone("zone-2", "Kitchen", ZoneState::Paused),
        ];

        let infos = to_zone_infos(&zones);

        assert_eq!(infos.len(), 2);
        assert_eq!(infos[0].zoneId, slint::SharedString::from("zone-1"));
        assert_eq!(
            infos[0].displayName,
            slint::SharedString::from("Living Room")
        );
        assert_eq!(infos[0].state, slint::SharedString::from("playing"));
        assert_eq!(infos[1].state, slint::SharedString::from("paused"));
    }
}
