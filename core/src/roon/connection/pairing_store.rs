//! Persists the pairing token a Roon Core issues on successful registration, so a relaunch of
//! this app doesn't have to re-Enable/re-pair from scratch. A small JSON file at an
//! XDG-appropriate path (Flatpak-safe, since Flatpak redirects `XDG_*` into the sandbox) — not
//! `rusqlite`, which isn't a dependency anywhere in this repo yet and was earmarked for the
//! separate, much larger album-art cache feature (TECH_STACK.md). Owned entirely by
//! `connection`, per CLAUDE.md §1 — `app` never touches this directly.
//!
//! A single saved pair, not a map keyed by `core_id`: multi-Core is a permanent non-goal
//! (NORTH-STAR.md), so there is never more than one relevant pairing to remember. Loading and
//! resending a token issued by a different Core than the one just discovered is safe either way
//! — `moo::handshake::register` only ever *offers* a token; the Core decides whether it
//! recognizes it, and simply falls back to treating the registration as unseen if not, the same
//! as sending no token at all.
//!
//! File I/O here is synchronous (`std::fs`), not `tokio::fs`: both `load` (once, before
//! `connection::run`'s loop starts) and `save` (once per successful registration, itself gated
//! on human approval and network round-trips taking whole seconds) are rare, small-file
//! operations — not worth the extra complexity of async file I/O for a sub-millisecond,
//! infrequent read/write on this app's single background-thread runtime.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The pairing token issued by the Core that last successfully registered this extension, plus
/// the MOO address that last succeeded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PairedCore {
    pub core_id: String,
    pub token: String,
    /// The `ws://<addr>/api` address used the last time registration succeeded — raced against a
    /// fresh SOOD discovery pass on process startup (`connection/mod.rs`'s `run`) as a
    /// bounded-timeout fast path, per CLAUDE.md's mandatory technical choices. `#[serde(default)]`
    /// so a file saved before this field existed still loads fine, just with nothing to race.
    #[serde(default)]
    pub cached_addr: Option<SocketAddr>,
}

/// The on-disk path this extension's pairing state lives at, or `None` if it can't be
/// determined (no home directory found for the current user — rare, but must degrade
/// gracefully to "don't persist this session" rather than panic).
pub(crate) fn default_path() -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("io.github", "hr46ph", "dissonanza")?;
    let state_dir = dirs.state_dir().unwrap_or_else(|| dirs.data_dir());
    Some(state_dir.join("pairing.json"))
}

/// Reads a previously-saved pairing token from `path`, or `None` if there isn't one yet, or the
/// file couldn't be read or parsed. Every failure mode collapses to the same `None` rather than
/// a typed error: the caller's fallback (register as unseen, same as a first-ever run) is always
/// safe and identical regardless of *why* a saved token isn't usable.
pub(crate) fn load(path: &Path) -> Option<PairedCore> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Writes `paired` to `path`, creating parent directories as needed. Returns `Err` on failure —
/// deliberately not swallowed here like `load`'s failures are, so a caller with somewhere useful
/// to report it (or tests) can see what went wrong; `connection::mod.rs`'s own caller still
/// treats it as non-fatal to the connection itself, matching `send_state`'s existing
/// publish-or-drop pattern elsewhere in this module — a local cache-file write failing must
/// never break an otherwise-working Roon connection.
pub(crate) fn save(path: &Path, paired: &PairedCore) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(paired).map_err(std::io::Error::other)?;
    std::fs::write(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A path under the OS temp dir, unique per test run (`std::process::id()` plus a
    /// monotonically-increasing counter) so parallel `cargo test` runs and repeat runs never
    /// collide — no new dev-dependency (e.g. `tempfile`) needed for a file this small.
    fn temp_path(label: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "dissonanza-test-pairing-{label}-{}-{n}.json",
            std::process::id()
        ))
    }

    #[test]
    fn round_trips_through_save_and_load() {
        let path = temp_path("round-trip");
        let paired = PairedCore {
            core_id: "core-1".to_string(),
            token: "tok-1".to_string(),
            cached_addr: None,
        };

        save(&path, &paired).expect("saves");
        let loaded = load(&path).expect("loads what was just saved");

        assert_eq!(loaded, paired);
        std::fs::remove_file(&path).expect("cleanup");
    }

    #[test]
    fn round_trips_a_cached_addr() {
        let path = temp_path("round-trip-addr");
        let paired = PairedCore {
            core_id: "core-1".to_string(),
            token: "tok-1".to_string(),
            cached_addr: Some("192.168.1.50:9330".parse().unwrap()),
        };

        save(&path, &paired).expect("saves");
        let loaded = load(&path).expect("loads what was just saved");

        assert_eq!(loaded, paired);
        std::fs::remove_file(&path).expect("cleanup");
    }

    #[test]
    fn load_defaults_cached_addr_when_absent_from_older_files() {
        let path = temp_path("no-cached-addr");
        std::fs::write(&path, br#"{"core_id":"core-1","token":"tok-1"}"#)
            .expect("write file without cached_addr, as an older version of this app would have");

        let loaded = load(&path).expect("loads despite the missing field");

        assert_eq!(
            loaded,
            PairedCore {
                core_id: "core-1".to_string(),
                token: "tok-1".to_string(),
                cached_addr: None,
            }
        );
        std::fs::remove_file(&path).expect("cleanup");
    }

    #[test]
    fn load_returns_none_when_the_file_does_not_exist() {
        let path = temp_path("missing");
        assert_eq!(load(&path), None);
    }

    #[test]
    fn load_returns_none_for_malformed_json() {
        let path = temp_path("malformed");
        std::fs::write(&path, b"not json").expect("write junk");

        assert_eq!(load(&path), None);
        std::fs::remove_file(&path).expect("cleanup");
    }

    #[test]
    fn save_creates_missing_parent_directories() {
        let path = temp_path("nested").parent().unwrap().join(format!(
            "dissonanza-test-nested-{}/pairing.json",
            std::process::id()
        ));
        let paired = PairedCore {
            core_id: "core-1".to_string(),
            token: "tok-1".to_string(),
            cached_addr: None,
        };

        save(&path, &paired).expect("creates parent dirs and saves");
        assert_eq!(load(&path), Some(paired));

        std::fs::remove_dir_all(path.parent().unwrap()).expect("cleanup");
    }
}
