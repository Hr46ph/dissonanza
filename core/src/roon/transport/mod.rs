//! `com.roonlabs.transport:2` — zone list, now-playing state, and playback control. Unlike
//! `connection`, this is a service the Core provides and this extension consumes: declared in
//! `connection`'s `REQUIRED_SERVICES` at registration, then driven entirely through
//! `connection::ConnectionRequests` (IMPL_TRANSPORT.md Phase 1) — this module never touches SOOD
//! discovery, pairing, or reconnect itself, per CLAUDE.md §1.
//!
//! IMPL_TRANSPORT.md Phase 2 covers zone subscription and the typed `Zone`/`Output`/`NowPlaying`
//! state model; Phase 3 (this module's `control` submodule) adds the `control`/`seek` playback
//! verbs; Phase 4 (same submodule) adds the `change_volume`/`mute`/`standby` output-control verbs.

mod control;
mod error;
mod model;
mod zones;

pub use control::{
    ChangeVolumeHow, ControlAction, MuteHow, SeekHow, change_volume, control, mute, seek, standby,
};
pub use error::TransportError;
pub use model::{
    LoopMode, NowPlaying, OneLine, Output, SourceControl, SourceControlStatus, ThreeLine, TwoLine,
    Volume, VolumeType, Zone, ZoneSettings, ZoneState,
};
pub use zones::{ZoneEvent, ZoneSeekChange, ZoneSubscription, subscribe_zones};
