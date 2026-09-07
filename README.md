# Dissonanza

A native Linux remote client for [Roon](https://roon.app), built entirely on Roon's officially
supported API.

## Status

Early development, not yet usable. The Roon connection layer (SOOD discovery, MOO protocol) is
in progress; the GUI hasn't started. See [CURRENT_STATE.md](CURRENT_STATE.md) for the current
module map and open work.

## Project layout

Cargo workspace with two crates:

- `core/` — Roon SOOD/MOO protocol client and all business logic, no GUI dependency.
- `app/` — the [Slint](https://slint.dev) GUI shell, depends only on `core`'s public API.

## Building

```sh
cargo build
```

Requires a recent stable Rust toolchain (2024 edition).

## Documentation

- [NORTH-STAR.md](NORTH-STAR.md) — long-term vision and non-goals.
- [TECH_STACK.md](TECH_STACK.md) — chosen stack and rationale.
- [CURRENT_STATE.md](CURRENT_STATE.md) — living project map.
- [RELEASES.md](RELEASES.md) — versioning, changelog, and release process.
- [docs/protocol/sood-moo.md](docs/protocol/sood-moo.md) — SOOD/MOO wire-protocol study.

## License

MIT — see [LICENSE](LICENSE).
