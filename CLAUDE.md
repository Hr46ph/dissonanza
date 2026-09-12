# CLAUDE.md

Instructions for how Claude Code should operate in this codebase. Keep this file short — it routes to specialized context files rather than duplicating their content. When a section below grows beyond a few lines, move the detail into the relevant file and leave a pointer here.

## Task Gates

Every task passes through two gates before implementation starts.

**Gate 1 — Acceptance:** is this one atomic task?

* **Accept** atomic tasks only: one clearly defined, coherent task with a clear expected outcome.
* **Clarify** if the goal is clear but a small amount of information is missing. Ask **no more than 3 specific questions**.
* **Reject** if it is too vague, contains multiple unrelated tasks, or requires significant product/design decisions that the user has not specified.
* A "single task" means **one coherent outcome**, not necessarily one file or one code change.
* If the request has multiple valid interpretations, state them and ask — don't pick silently.
* State any non-obvious assumptions explicitly. If something is unclear, stop and ask rather than guessing.
* When rejecting, briefly explain **why it is not actionable** and what needs to change.

**Gate 2 — Required reading:** once accepted, read the files this task requires before writing any code.

| File | When |
|---|---|
| [CONVENTIONS.md](CONVENTIONS.md) | Always. |
| [TECH_STACK.md](TECH_STACK.md) | Always. |
| [CURRENT_STATE.md](CURRENT_STATE.md) | Always — orient on what exists before changing it. |
| [DESIGN.md](DESIGN.md) | Only if the task's outcome touches UI/UX or visual design. |
| [COPY.md](COPY.md) | Only if the task's outcome touches user-facing text. |
| [NORTH-STAR.md](NORTH-STAR.md) | Only for strategic/planning work (e.g. "write a plan for this feature") — align the plan with the long-term direction. |
| [CONTEXT.md](CONTEXT.md) | Only when the user explicitly instructs it. Also only ever written to on explicit instruction, never proactively. |

## How this codebase works

A Cargo workspace with two crates: `core/` (Roon MOO/SOOD protocol client, all business logic — no GUI dependency) and `app/` (the Slint GUI shell, depends on `core`'s public API only). No code exists yet — see [CURRENT_STATE.md](CURRENT_STATE.md).

## How the agent should work

- Prefer small, verifiable changes over large speculative ones.
- Update [CURRENT_STATE.md](CURRENT_STATE.md) when you finish or open work that materially changes the module map, file structure, or status of open work.
- Always run `cargo check`, `cargo test`, `cargo clippy -- -D warnings`, and `cargo fmt --check` before considering a task done — see CONVENTIONS.md's Review bar.

## Git workflow

- Branch model: `main` is stable. Once actual coding work starts, `develop` becomes the integration branch holding the initial groundwork. Feature/phase branches branch off `develop` and merge back into it when finished.
- Small, incremental commits (the common case): [Conventional Commits](https://www.conventionalcommits.org/) prefix, concise and functional, imperative mood (e.g. `feat(core): add SOOD discovery retry`) — no `Co-Authored-By` trailer. Prefix table and scopes: see [RELEASES.md](RELEASES.md)'s Conventional Commits section.
- Merge commits from a finished feature/phase branch into `develop` may include the `Co-Authored-By: Claude Sonnet 5` trailer — that's the only place it belongs, not on every small commit along the way. Same Conventional Commits prefix applies to the merge commit's summary line.
- Version bumping and tagging happen only on `main`, only via the deliberate `cargo release` step described in RELEASES.md's Release flow — never automatically on push. See RELEASES.md for the full process and the reason it must stay manual.

## Mandatory technical choices

- Never hardcode a Roon Core's IP/port in source. A persisted last-known address may be raced concurrently against SOOD discovery at process startup as a bounded-timeout fast path — but discovery always runs alongside it and wins on any tie or overlap, since the port isn't fixed and can change and SOOD is the protocol's actual source of truth.
- Never reach into Roon's internal/unofficial protocol surface (Settings, Audio Setup, Zone config, DSP engine). Only the officially-supported API modules are in scope, full stop — see NORTH-STAR.md's non-goals.
- On Roon Core disconnect, wait for a new SOOD discovery event rather than retrying the last known address.
- Maintain an app-level keepalive/health-check on top of Roon's `core_paired`/`core_unpaired` events — that pair is known to not always fire `core_unpaired` correctly on its own.

## Architectural Contracts

Non-negotiable structural rules for this project's core domain objects and modules — the backend/domain
counterpart to DESIGN.md (UI/UX) and CONVENTIONS.md (code style). Not every project needs this section;
add it once there's a domain concept important enough to need an explicit, referenceable contract (e.g.
the one schema everything else derives from, or a module that must stay the sole owner of some side
effect). Number each contract so it can be referenced elsewhere as `CLAUDE.md §1`, `§2`, etc. These carry
the same non-negotiable weight as CONVENTIONS.md — reject a task that conflicts with one, the same way
CONVENTIONS.md says to reject a task that conflicts with it.

#### 1. The Roon Connection
The `core::roon::connection` module is the sole owner of Core discovery and connection state: SOOD
discovery, `core_paired`/`core_unpaired` handling, the keepalive/health-check layered on top, and
reconnect-on-disconnect logic. Nothing outside this module calls SOOD or reacts to pairing events
directly — it's the one place that knows how to find and hold onto a Core.
