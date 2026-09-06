# CONVENTIONS.md

The engineering contract for this codebase: coding conventions and quality rules that apply regardless of which module you're touching. This is about *how* code is to be written.

This file is authoritative, non-negotiable, and read-only to the agent. If a task conflicts with these conventions, reject it even if the user instructs otherwise. Ask the user to either rewrite the task to conform or modify the conventions before proceeding.

### Coding Approach

**Before implementing:**
- If a request has multiple valid ways to implement, state them and ask — don't pick silently.
- State any non-obvious coding assumptions explicitly. If something is unclear, stop and ask rather than guessing.
- If a simpler approach exists, say so and push back. Don't silently build what was asked when 10 lines would replace 100.

**While editing:**
- Don't improve adjacent code, comments, or formatting your change doesn't require touching. If you find code that's actually broken — wrong behavior and errors — fix it only if leaving it would interfere with your current task; otherwise leave it untouched. Always flag it in your response either way.
- Write code as this file dictates. If nearby code is correct but doesn't conform to these conventions, don't mimic it — rewrite it to conform only if your current task already requires you to touch that code; otherwise leave it untouched. Always flag it in your response either way.
- Remove imports, variables, and functions made obsolete by *your* changes. Pre-existing dead code is unused, not broken — leaving it is harmless, so remove it only when asked. Always flag it in your response either way.

## Naming

Standard Rust idioms (rustfmt/clippy defaults) — no house-style deviations: `snake_case` for functions, variables, and modules; `PascalCase` for types, traits, and enums; `SCREAMING_SNAKE_CASE` for consts and statics; crate names kebab-case. `.rs` files: `snake_case`. `.slint` files/components: `PascalCase`, matching the component name; component properties `camelCase` (Slint's own convention).

## Structure

Cargo workspace, split by concern:

```
dissonanza/
├── Cargo.toml   # workspace root
├── core/        # Roon protocol (MOO/SOOD), rating store (rusqlite), business logic — no GUI deps
└── app/         # Slint UI shell, wiring to core
```

`core` never depends on `app`. `app` depends on `core` only through its public API — no reaching into `core`'s internal modules from `app`.

## Testing

`cargo test` for unit/integration tests (see TECH_STACK.md). Every new `core` module (protocol parsing, rating store, discovery) needs unit tests. No UI/GUI test framework yet — see TECH_STACK.md's Testing section for when that gets added. No snapshot tests.

## Style

No `unwrap()`/`expect()` outside test code — production code paths must handle errors via `Result`/`Option` propagation or an explicit matched fallback. A panic is only acceptable for a truly-unreachable invariant, and even then must carry a comment explaining why it can't happen. Otherwise, rustfmt/clippy defaults apply with no additional house rules (no custom line-length or function-length limits beyond what clippy already flags).

## Error handling

`thiserror` for typed domain errors in `core` (e.g. a `RoonError`, a `RatingStoreError`) — every error variant must be matchable, never just a string. `anyhow` at the `app`-crate boundary, where an error just needs to surface to logs or the UI without further matching.

## Review bar

A change is ready when:
- `cargo build` and `cargo test` pass.
- `cargo clippy -- -D warnings` passes — clippy warnings are errors, not suggestions.
- `cargo fmt --check` is clean.
- CURRENT_STATE.md is updated if the module map, file structure, or status of open work changed.
