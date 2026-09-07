// Visible to `connection` (not further) so `connection/mod.rs` can drive them — `moo` itself
// stays a private module, so nothing outside `connection` can reach it directly, per CLAUDE.md
// §1.
pub(super) mod handshake;
pub(super) mod message;
pub(super) mod transport;
