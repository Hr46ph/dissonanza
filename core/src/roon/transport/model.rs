//! Typed `com.roonlabs.transport:2` data model — `Zone`, `Output`, and their nested types — per
//! `docs/protocol/transport.md`'s "Data model" section. Pure `serde::Deserialize` types, no I/O;
//! parsed out of `Subscribed`/`Changed` bodies by `super::zones`.

use serde::Deserialize;

/// Shared by both [`Zone::state`] and [`Output::state`], per the wire study.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoneState {
    Playing,
    Paused,
    Loading,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Zone {
    pub zone_id: String,
    pub display_name: String,
    #[serde(default)]
    pub outputs: Vec<Output>,
    pub state: ZoneState,
    #[serde(default)]
    pub seek_position: Option<f64>,
    pub is_previous_allowed: bool,
    pub is_next_allowed: bool,
    pub is_pause_allowed: bool,
    pub is_play_allowed: bool,
    pub is_seek_allowed: bool,
    #[serde(default)]
    pub queue_items_remaining: Option<u64>,
    #[serde(default)]
    pub queue_time_remaining: Option<f64>,
    pub settings: ZoneSettings,
    #[serde(default)]
    pub now_playing: Option<NowPlaying>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ZoneSettings {
    #[serde(rename = "loop")]
    pub loop_mode: LoopMode,
    pub shuffle: bool,
    pub auto_radio: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopMode {
    Loop,
    LoopOne,
    Disabled,
}

/// Present on [`Zone::now_playing`] only while playback is active. No raw artist/album/track
/// fields exist here — a UI wanting those parses the pre-formatted `*_line` strings, or uses
/// `browse:1`/an image service, both out of scope for this module.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NowPlaying {
    #[serde(default)]
    pub seek_position: Option<f64>,
    #[serde(default)]
    pub length: Option<f64>,
    #[serde(default)]
    pub image_key: Option<String>,
    pub one_line: OneLine,
    pub two_line: TwoLine,
    pub three_line: ThreeLine,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct OneLine {
    pub line1: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TwoLine {
    pub line1: String,
    #[serde(default)]
    pub line2: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ThreeLine {
    pub line1: String,
    #[serde(default)]
    pub line2: Option<String>,
    #[serde(default)]
    pub line3: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Output {
    pub output_id: String,
    pub zone_id: String,
    pub display_name: String,
    /// Optional despite the JSDoc and docs/protocol/transport.md's original study both listing it
    /// as required (same as `Zone::state`) — confirmed missing entirely on a live Core's output
    /// whose only `source_controls` entry was `"status": "indeterminate"` (2026-09-11), unlike
    /// `Zone::state`, which was present in that same body.
    #[serde(default)]
    pub state: Option<ZoneState>,
    /// An array despite the JSDoc's singular formatting — see docs/protocol/transport.md's
    /// "Rust design notes" for why the array reading is the correct one.
    #[serde(default)]
    pub source_controls: Vec<SourceControl>,
    #[serde(default)]
    pub volume: Option<Volume>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SourceControl {
    pub control_key: String,
    pub display_name: String,
    pub status: SourceControlStatus,
    pub supports_standby: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceControlStatus {
    Selected,
    Deselected,
    Standby,
    Indeterminate,
}

/// `min`/`max`/`value`/`step` are floats per the wire study's explicit note, not integers — one of
/// the two documented divergences from `TheAppgineer/rust-roon-api`. `type: "incremental"` means a
/// +/- only control with no range feedback at all, so every field but `kind` is `None` for it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Volume {
    #[serde(rename = "type")]
    pub kind: VolumeType,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub value: Option<f64>,
    #[serde(default)]
    pub step: Option<f64>,
    #[serde(default)]
    pub is_muted: Option<bool>,
}

/// An unrecognized `type` should be treated like `Number`, per the wire study — modeled as a
/// catch-all `Other` variant rather than preserving the raw string, since nothing in this phase
/// needs to distinguish one unrecognized type from another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VolumeType {
    Number,
    Db,
    Incremental,
    #[serde(other)]
    Other,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_zone_json() -> serde_json::Value {
        serde_json::json!({
            "zone_id": "zone-1",
            "display_name": "Living Room",
            "outputs": [{
                "output_id": "output-1",
                "zone_id": "zone-1",
                "display_name": "Living Room",
                "state": "playing",
                "source_controls": [{
                    "control_key": "src-1",
                    "display_name": "Optical",
                    "status": "selected",
                    "supports_standby": true,
                }],
                "volume": {
                    "type": "db",
                    "min": -80.0,
                    "max": 10.0,
                    "value": -20.5,
                    "step": 0.5,
                    "is_muted": false,
                },
            }],
            "state": "playing",
            "seek_position": 12.0,
            "is_previous_allowed": true,
            "is_next_allowed": true,
            "is_pause_allowed": true,
            "is_play_allowed": false,
            "is_seek_allowed": true,
            "queue_items_remaining": 5,
            "queue_time_remaining": 300.0,
            "settings": {
                "loop": "loop_one",
                "shuffle": false,
                "auto_radio": true,
            },
            "now_playing": {
                "seek_position": 12.0,
                "length": 240.0,
                "image_key": "img-1",
                "one_line": { "line1": "Track — Artist" },
                "two_line": { "line1": "Track", "line2": "Artist" },
                "three_line": { "line1": "Track", "line2": "Artist", "line3": "Album" },
            },
        })
    }

    #[test]
    fn parses_a_fully_populated_zone() {
        let zone: Zone = serde_json::from_value(full_zone_json()).expect("parses");

        assert_eq!(zone.zone_id, "zone-1");
        assert_eq!(zone.state, ZoneState::Playing);
        assert_eq!(zone.settings.loop_mode, LoopMode::LoopOne);
        assert!(!zone.settings.shuffle);

        let output = &zone.outputs[0];
        assert_eq!(
            output.source_controls[0].status,
            SourceControlStatus::Selected
        );
        let volume = output.volume.as_ref().expect("volume present");
        assert_eq!(volume.kind, VolumeType::Db);
        assert_eq!(volume.value, Some(-20.5));

        let now_playing = zone.now_playing.as_ref().expect("now_playing present");
        assert_eq!(now_playing.two_line.line2.as_deref(), Some("Artist"));
        assert_eq!(now_playing.three_line.line3.as_deref(), Some("Album"));
    }

    #[test]
    fn parses_a_minimal_stopped_zone_with_no_now_playing_or_outputs() {
        let value = serde_json::json!({
            "zone_id": "zone-2",
            "display_name": "Kitchen",
            "state": "stopped",
            "is_previous_allowed": false,
            "is_next_allowed": false,
            "is_pause_allowed": false,
            "is_play_allowed": true,
            "is_seek_allowed": false,
            "settings": { "loop": "disabled", "shuffle": false, "auto_radio": false },
        });

        let zone: Zone = serde_json::from_value(value).expect("parses");

        assert!(zone.outputs.is_empty());
        assert_eq!(zone.now_playing, None);
        assert_eq!(zone.seek_position, None);
        assert_eq!(zone.queue_items_remaining, None);
    }

    #[test]
    fn output_state_is_optional() {
        // The exact shape observed on a live Core (2026-09-11): an output whose only
        // source_controls entry is "indeterminate" carries no `state` field at all, unlike its
        // parent zone, which does.
        let mut value = full_zone_json();
        value["outputs"][0].as_object_mut().unwrap().remove("state");
        value["outputs"][0]["source_controls"][0]["status"] = serde_json::json!("indeterminate");

        let zone: Zone = serde_json::from_value(value).expect("parses without output.state");

        assert_eq!(
            zone.state,
            ZoneState::Playing,
            "the zone's own state is unaffected"
        );
        assert_eq!(zone.outputs[0].state, None);
    }

    #[test]
    fn incremental_volume_has_no_range_fields() {
        let value = serde_json::json!({ "type": "incremental" });

        let volume: Volume = serde_json::from_value(value).expect("parses");

        assert_eq!(volume.kind, VolumeType::Incremental);
        assert_eq!(volume.min, None);
        assert_eq!(volume.value, None);
    }

    #[test]
    fn unrecognized_volume_type_falls_back_to_other() {
        let value = serde_json::json!({ "type": "some_future_type" });

        let volume: Volume = serde_json::from_value(value).expect("parses");

        assert_eq!(volume.kind, VolumeType::Other);
    }

    #[test]
    fn ignores_unknown_fields_on_zone_and_output() {
        let mut value = full_zone_json();
        value["extra_unknown_field"] = serde_json::json!("ignored");
        value["outputs"][0]["another_unknown_field"] = serde_json::json!(42);

        serde_json::from_value::<Zone>(value).expect("unknown fields don't break parsing");
    }
}
