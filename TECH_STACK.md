# TECH_STACK.md

The opinionated, explicitly chosen stack for this project. Once a choice is recorded here, treat it as settled — don't re-propose alternatives unless the user raises it. This prevents the agent from "helpfully" suggesting a different framework/library every session.

## Language & runtime

Rust (core/backend). Async I/O via `tokio` for the Roon MOO/websocket protocol and the custom SOOD discovery implementation. `rayon` for multicore image decode/resize (album art).

## Framework

Slint — a Rust-native, declarative GUI framework (`.slint` markup) with its own GPU-accelerated renderer (Skia/femtovg backends on Linux). No FFI boundary and no second language/toolchain: everything — UI and core — stays in Rust, so compiler and clippy feedback stays actionable across the whole app.

Slint's desktop platform support is officially "in progress" upstream, but judged mature enough here: this is a single-window control-plane app, not one doing deep OS shell integration (custom native menu bars, tray-only modes, etc.).

## Data / storage

`rusqlite` (SQLite) for local cache metadata and track ratings.

## Testing

- `cargo test` for unit/integration tests.
- `cargo check` and `cargo build` must pass before any change is considered done.
- `cargo clippy -- -D warnings` — clippy warnings are treated as errors to fix, not suggestions to ignore.
- `cargo fmt --check` — formatting is enforced automatically, not manually reviewed.
- No dedicated UI test framework yet. Add one (e.g. Slint's snapshot/test tooling) once the GUI surface is large enough to justify it.

## Infrastructure / deployment

- **Flatpak (Flathub)** — primary distribution channel. One package for all distros; standard network permissions cover UDP multicast (SOOD); GPU passthrough (Vulkan/GL) via standard extensions.
- **AUR** — secondary channel, for Arch users avoiding Flatpak.
- **AppImage** — optional, "try without installing" only. No sandboxing/update mechanism needed alongside Flatpak.

## Explicitly rejected

- **Qt6/QML + `cxx-qt`** — the original candidate framework, reconsidered in favor of Slint: introduces an FFI boundary and a second language/toolchain (C++/CMake) for no benefit over Slint's own GPU-accelerated renderer, for an app this size.
- **Electron/web tech** — conflicts with the latency requirement.
- **Relying on an existing Rust Roon-API crate for MOO/SOOD** — building a custom implementation instead, "to be safe."
- **Self-maintained .deb/.rpm repo** — maintenance burden not worth it vs. Flatpak.
