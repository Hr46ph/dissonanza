//! `com.roonlabs.transport:2` — zone list, now-playing state, and playback control. Unlike
//! `connection`, this is a service the Core provides and this extension consumes: declared in
//! `connection`'s `REQUIRED_SERVICES` at registration, then driven entirely through
//! `connection::ConnectionRequests` (IMPL_TRANSPORT.md Phase 1) — this module never touches SOOD
//! discovery, pairing, or reconnect itself, per CLAUDE.md §1.
//!
//! IMPL_TRANSPORT.md Phase 2 covers zone subscription and the typed `Zone`/`Output`/`NowPlaying`
//! state model (this module, so far); playback and volume/output control verbs land in later
//! phases.

mod error;
mod model;
mod zones;

pub use error::TransportError;
pub use model::{
    LoopMode, NowPlaying, OneLine, Output, SourceControl, SourceControlStatus, ThreeLine, TwoLine,
    Volume, VolumeType, Zone, ZoneSettings, ZoneState,
};
pub use zones::{ZoneEvent, ZoneSeekChange, ZoneSubscription, subscribe_zones};
