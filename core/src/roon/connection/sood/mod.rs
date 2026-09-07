// `discovery` is visible to `connection` (not further) so `connection/mod.rs` can drive it —
// `sood` itself stays a private module, so nothing outside `connection` can reach it directly,
// per CLAUDE.md §1.
pub(super) mod discovery;
mod message;
