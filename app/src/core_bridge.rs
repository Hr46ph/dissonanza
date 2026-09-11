//! Bridges `core::roon::connection`'s `tokio`-async, channel-based API onto Slint's own blocking
//! UI event loop (IMPL_UI_SHELL.md Phase 1's open architectural question). Owns a background OS
//! thread running a dedicated `tokio` runtime that drives `Connection::spawn`; forwards
//! `ConnectionEvent`s to the UI thread via `slint::invoke_from_event_loop`. `.slint` files stay
//! pure view — no `core` types or async logic are reachable from them directly.

use std::thread;

use dissonanza_core::roon::connection::{
    Connection, ConnectionConfig, ConnectionEvent, ConnectionRequests, ConnectionState,
};
use dissonanza_core::roon::transport::{
    self, ChangeVolumeHow, ControlAction, MuteHow, SeekHow, TransportError, Zone, ZoneEvent,
    ZoneSubscription,
};
use slint::{ModelRc, VecModel, Weak};
use tokio::sync::mpsc;

use crate::{AppWindow, ZoneInfo};

/// A UI-originated playback command — the first **UI → core** direction this bridge carries (every
/// prior phase only pushed `core` state onto the UI thread). Sent over an `mpsc` channel from
/// wherever `.slint` callbacks are wired (IMPL_UI_SHELL.md Phase 3.2-3.4), dispatched by
/// [`drive_connection`]'s `tokio::select!` loop the same way inbound `ConnectionEvent`s/`ZoneEvent`s
/// already are, just reversed. No `SelectZone` variant: which zone is selected is tracked entirely
/// UI-side (`AppWindow`'s `selectedZoneId` property, per Phase 2) and each command below already
/// carries the zone/output id it targets.
///
/// `PauseAll` (added alongside the zone-switcher popup's "Pause all" row, DESIGN.md's Dropdown/menu
/// entry) has no target id — it's handled specially in `drive_connection`'s `command` arm rather than
/// by [`dispatch_command`], since only the loop itself holds the current `zones: Vec<Zone>` needed to
/// fan it out to every zone.
///
/// `Standby` carries a temporary `#[allow(dead_code)]`: unlike `core` (a lib crate, where `pub` items
/// are reachable API regardless of in-crate callers), `app` is a bin crate, and no numbered step in
/// IMPL_UI_SHELL.md's Phase 3 wires a `.slint` control that constructs it — every other variant now
/// has a caller (`Control`/`Seek`/`PauseAll` since Phase 3.2/3.3, `ChangeVolume`/`Mute` since Phase
/// 3.4's volume popover).
#[derive(Debug, Clone)]
pub enum BridgeCommand {
    Control {
        zone_or_output_id: String,
        action: ControlAction,
    },
    Seek {
        zone_or_output_id: String,
        how: SeekHow,
        seconds: f64,
    },
    PauseAll,
    ChangeVolume {
        output_id: String,
        how: ChangeVolumeHow,
        value: f64,
    },
    Mute {
        output_id: String,
        how: MuteHow,
    },
    #[allow(dead_code)]
    Standby {
        output_id: String,
        control_key: Option<String>,
    },
}

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
/// IMPL_UI_SHELL.md). Returns the sender half of the `BridgeCommand` channel (IMPL_UI_SHELL.md
/// Phase 3.1) — the caller must keep it alive for as long as commands should be deliverable;
/// dropping it ends `drive_connection`'s loop the same way the `ConnectionEvent` channel closing
/// does.
pub fn spawn(ui: Weak<AppWindow>) -> mpsc::UnboundedSender<BridgeCommand> {
    let (commands_tx, commands_rx) = mpsc::unbounded_channel();
    thread::spawn(move || run(ui, commands_rx));
    commands_tx
}

fn run(ui: Weak<AppWindow>, commands: mpsc::UnboundedReceiver<BridgeCommand>) {
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
    runtime.block_on(drive_connection(ui, commands));
}

async fn drive_connection(
    ui: Weak<AppWindow>,
    mut commands: mpsc::UnboundedReceiver<BridgeCommand>,
) {
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
            command = commands.recv() => {
                let Some(command) = command else { break };
                // `PauseAll` fans out to every currently-known zone — handled here rather than in
                // `dispatch_command`, since only this loop holds `zones`.
                if matches!(command, BridgeCommand::PauseAll) {
                    for zone in &zones {
                        dispatch_command(
                            &requests,
                            BridgeCommand::Control {
                                zone_or_output_id: zone.zone_id.clone(),
                                action: ControlAction::Pause,
                            },
                        )
                        .await;
                    }
                } else {
                    dispatch_command(&requests, command).await;
                }
            }
        }
    }
}

/// Sends one `BridgeCommand` as the matching `core::roon::transport::control` request and waits for
/// it to complete. Failures are logged, not surfaced to the UI — `core_bridge` has no error-display
/// mechanism yet, per IMPL_UI_SHELL.md Phase 3's design notes; building one is out of scope here.
async fn dispatch_command(requests: &ConnectionRequests, command: BridgeCommand) {
    let result = match command {
        BridgeCommand::Control {
            zone_or_output_id,
            action,
        } => transport::control(requests, &zone_or_output_id, action).await,
        BridgeCommand::Seek {
            zone_or_output_id,
            how,
            seconds,
        } => transport::seek(requests, &zone_or_output_id, how, seconds).await,
        // Filtered out by the caller before reaching here (see `drive_connection`'s `command` arm) —
        // handled as a no-op rather than an unreachable panic, so a future direct call stays safe.
        BridgeCommand::PauseAll => Ok(()),
        BridgeCommand::ChangeVolume {
            output_id,
            how,
            value,
        } => transport::change_volume(requests, &output_id, how, value).await,
        BridgeCommand::Mute { output_id, how } => transport::mute(requests, &output_id, how).await,
        BridgeCommand::Standby {
            output_id,
            control_key,
        } => transport::standby(requests, &output_id, control_key.as_deref()).await,
    };
    if let Err(err) = result {
        eprintln!("bridge command failed: {err}");
    }
}

/// Maps a `.slint`-originated action string — Roon's own wire vocabulary
/// ("play"/"pause"/"playpause"/"previous"/"next", matching `ControlAction`'s `serde` renames — see
/// `AppWindow.slint`'s `controlRequested` callback) — to `ControlAction`. `None` for anything
/// unrecognized; `main.rs` drops the command rather than sending a malformed one.
pub fn parse_control_action(action: &str) -> Option<ControlAction> {
    match action {
        "play" => Some(ControlAction::Play),
        "pause" => Some(ControlAction::Pause),
        "playpause" => Some(ControlAction::PlayPause),
        "stop" => Some(ControlAction::Stop),
        "previous" => Some(ControlAction::Previous),
        "next" => Some(ControlAction::Next),
        _ => None,
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

/// Maps each `Zone` to a `.slint`-bindable `ZoneInfo`, including its first output's volume/mute state
/// (IMPL_UI_SHELL.md Phase 3.4's design notes: a grouped zone's volume popover always drives its
/// first output, never a per-output picker). `hasVolume` is false — and `volumeMin`/`volumeMax`/
/// `volumeValue`/`isMuted` are left at their zero/false defaults, unused by `.slint` in that case —
/// whenever the zone has no outputs, the first output has no `Volume` at all, or that `Volume`'s
/// `min`/`max`/`value` aren't all present (the `incremental` type, per `transport::model::Volume`'s
/// own doc comment, leaves every field but `kind` as `None`).
fn to_zone_infos(zones: &[Zone]) -> Vec<ZoneInfo> {
    zones
        .iter()
        .map(|zone| {
            let output = zone.outputs.first();
            let volume = output.and_then(|output| output.volume.as_ref());
            let has_volume = volume.is_some_and(|volume| {
                volume.min.is_some() && volume.max.is_some() && volume.value.is_some()
            });
            ZoneInfo {
                zoneId: zone.zone_id.clone().into(),
                displayName: zone.display_name.clone().into(),
                state: describe_zone_state(zone.state).into(),
                outputId: output
                    .map(|output| output.output_id.clone())
                    .unwrap_or_default()
                    .into(),
                hasVolume: has_volume,
                volumeMin: volume.and_then(|volume| volume.min).unwrap_or(0.0) as f32,
                volumeMax: volume.and_then(|volume| volume.max).unwrap_or(0.0) as f32,
                volumeValue: volume.and_then(|volume| volume.value).unwrap_or(0.0) as f32,
                isMuted: volume.and_then(|volume| volume.is_muted).unwrap_or(false),
            }
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
    use dissonanza_core::roon::transport::{
        LoopMode, Output, Volume, VolumeType, ZoneSeekChange, ZoneSettings, ZoneState,
    };

    use super::*;

    fn zone(id: &str, name: &str, state: ZoneState) -> Zone {
        zone_with_outputs(id, name, state, Vec::new())
    }

    fn zone_with_outputs(id: &str, name: &str, state: ZoneState, outputs: Vec<Output>) -> Zone {
        Zone {
            zone_id: id.to_string(),
            display_name: name.to_string(),
            outputs,
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

    fn output(id: &str, volume: Option<Volume>) -> Output {
        Output {
            output_id: id.to_string(),
            zone_id: "zone-1".to_string(),
            display_name: id.to_string(),
            state: Some(ZoneState::Playing),
            source_controls: Vec::new(),
            volume,
        }
    }

    #[test]
    fn maps_the_first_outputs_volume_onto_zone_info() {
        let zones = vec![zone_with_outputs(
            "zone-1",
            "Living Room",
            ZoneState::Playing,
            vec![output(
                "output-1",
                Some(Volume {
                    kind: VolumeType::Number,
                    min: Some(0.0),
                    max: Some(100.0),
                    value: Some(40.0),
                    step: Some(1.0),
                    is_muted: Some(true),
                }),
            )],
        )];

        let infos = to_zone_infos(&zones);

        assert!(infos[0].hasVolume);
        assert_eq!(infos[0].outputId, slint::SharedString::from("output-1"));
        assert_eq!(infos[0].volumeMin, 0.0);
        assert_eq!(infos[0].volumeMax, 100.0);
        assert_eq!(infos[0].volumeValue, 40.0);
        assert!(infos[0].isMuted);
    }

    #[test]
    fn zone_info_has_no_volume_without_an_output() {
        let zones = vec![zone("zone-1", "Living Room", ZoneState::Playing)];

        let infos = to_zone_infos(&zones);

        assert!(!infos[0].hasVolume);
        assert_eq!(infos[0].outputId, slint::SharedString::from(""));
    }

    #[test]
    fn zone_info_has_no_volume_for_an_incremental_only_control() {
        let zones = vec![zone_with_outputs(
            "zone-1",
            "Living Room",
            ZoneState::Playing,
            vec![output(
                "output-1",
                Some(Volume {
                    kind: VolumeType::Incremental,
                    min: None,
                    max: None,
                    value: None,
                    step: None,
                    is_muted: None,
                }),
            )],
        )];

        let infos = to_zone_infos(&zones);

        assert!(!infos[0].hasVolume);
    }

    #[test]
    fn parses_recognized_action_strings() {
        assert_eq!(parse_control_action("play"), Some(ControlAction::Play));
        assert_eq!(parse_control_action("pause"), Some(ControlAction::Pause));
        assert_eq!(
            parse_control_action("playpause"),
            Some(ControlAction::PlayPause)
        );
        assert_eq!(parse_control_action("stop"), Some(ControlAction::Stop));
        assert_eq!(
            parse_control_action("previous"),
            Some(ControlAction::Previous)
        );
        assert_eq!(parse_control_action("next"), Some(ControlAction::Next));
    }

    #[test]
    fn rejects_an_unrecognized_action_string() {
        assert_eq!(parse_control_action("rewind"), None);
    }
}
