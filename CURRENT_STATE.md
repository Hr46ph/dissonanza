# CURRENT_STATE.md

Living project map. Update when the project's structure, capabilities, or known open work materially changes — it's the fastest way for a new session (human or agent) to get oriented without re-reading the whole codebase. Not an obligation to log every implementation detail: a renamed variable or a one-line tweak isn't material, a new module or a capability going from "planned" to "done" is.

Keep entries short. This file describes *what is*, not *how it should be built* (that's [CONVENTIONS.md](CONVENTIONS.md)/[TECH_STACK.md](TECH_STACK.md)) or *where it's headed* (that's [NORTH-STAR.md](NORTH-STAR.md)).

## File Structure

No Rust or Slint code exists yet — the repository is currently documentation/scaffolding only:

```
dissonanza/
├── CLAUDE.md           # agent operating instructions (local-only, gitignored)
├── CONTEXT.md          # debugging memory, written only on explicit instruction
├── CONVENTIONS.md      # engineering conventions
├── COPY.md             # tone/voice/terminology
├── CURRENT_STATE.md    # this file
├── DESIGN.md           # visual/UX spec (local-only, gitignored)
├── NORTH-STAR.md       # long-term vision
├── TECH_STACK.md       # chosen stack
├── docs/
│   └── roon-linux-remote-client-onderzoek.md   # original research notes (Dutch); source for the files above
└── README.md
```

## Modules

None yet — no code has been written.

## Open work

- Core Rust backend (Roon MOO/SOOD client, SQLite rating store) and Slint GUI: both not yet started.
- Track identity for local ratings undecided: file path vs. MusicBrainz ID vs. Roon's `item_key` (the latter isn't guaranteed stable across rescans/moves).
- Cross-installation rating sync mechanism undecided.
- Custom Rust SOOD/MOO protocol implementation needs its own reverse-engineering/documentation study — no existing crate is being relied on.
- Cache invalidation strategy for locally cached album art (when to refresh on a new Roon scan or changed art) undecided.
- DESIGN.md's color tokens, typography scale, and concrete component states are marked TBD pending user-supplied screenshots of Roon's official UI.

## Recently changed

- Project scaffolding filled in: TECH_STACK.md, NORTH-STAR.md, DESIGN.md, CURRENT_STATE.md, CONVENTIONS.md, COPY.md, and CLAUDE.md's open sections, drafted from `docs/roon-linux-remote-client-onderzoek.md` plus follow-up decisions with the user (2026-09-06). CONTEXT.md intentionally left untouched (only ever written on explicit instruction).
