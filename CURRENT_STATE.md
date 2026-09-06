# CURRENT_STATE.md

Living project map. Update when the project's structure, capabilities, or known open work materially changes — it's the fastest way for a new session (human or agent) to get oriented without re-reading the whole codebase. Not an obligation to log every implementation detail: a renamed variable or a one-line tweak isn't material, a new module or a capability going from "planned" to "done" is.

Keep entries short. This file describes *what is*, not *how it should be built* (that's [CONVENTIONS.md](CONVENTIONS.md)/[TECH_STACK.md](TECH_STACK.md)) or *where it's headed* (that's [NORTH-STAR.md](NORTH-STAR.md)).

## File Structure

No Rust or Slint code exists yet — the repository is currently documentation/scaffolding only:

```
dissonanza/
├── CLAUDE.md           # agent operating instructions
├── cliff.toml          # git-cliff config: generates CHANGELOG.md from Conventional Commits
├── CONTEXT.md          # debugging memory, written only on explicit instruction
├── CONVENTIONS.md      # engineering conventions
├── COPY.md             # tone/voice/terminology
├── CURRENT_STATE.md    # this file
├── DESIGN.md           # visual/UX spec
├── NORTH-STAR.md       # long-term vision
├── RELEASES.md         # versioning/release/CI/packaging process
├── TECH_STACK.md       # chosen stack
├── .github/
│   └── workflows/
│       ├── ci.yml       # format/lint/test on every push/PR, version-bump guardrail on PRs into main
│       └── release.yml  # binary build, Flatpak validation, GitHub Release, AUR push on tag
├── packaging/
│   ├── flatpak/
│   │   ├── io.github.hr46ph.Dissonanza.yml            # Flatpak manifest (draft, see RELEASES.md gaps)
│   │   ├── io.github.hr46ph.Dissonanza.desktop         # desktop entry
│   │   └── io.github.hr46ph.Dissonanza.metainfo.xml    # AppStream metadata (draft)
│   └── aur/
│       └── PKGBUILD    # AUR package recipe (draft, checksums pending a real tag)
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
- CI (`ci.yml`) will fail until the Cargo workspace exists — expected, not a bug, per RELEASES.md.
- Flatpak submission is blocked on making the repo public (currently private, see TECH_STACK.md) and on generating `cargo-sources.json` once real dependencies are locked in.
- AUR push in `release.yml` is scaffolded but inert until `AUR_SSH_PRIVATE_KEY`/`AUR_USERNAME`/`AUR_EMAIL` secrets are configured, and until the package is claimed on AUR with a first manual push.
- `PKGBUILD` checksums are placeholders (`SKIP`) until a real `v0.1.0` tag exists.

## Recently changed

- Project scaffolding filled in: TECH_STACK.md, NORTH-STAR.md, DESIGN.md, CURRENT_STATE.md, CONVENTIONS.md, COPY.md, and CLAUDE.md's open sections, drafted from `docs/roon-linux-remote-client-onderzoek.md` plus follow-up decisions with the user (2026-09-06). CONTEXT.md intentionally left untouched (only ever written on explicit instruction).
- Standardized versioning, changelog, and release-cutting (2026-09-06): Conventional Commits adopted for all commit messages (RELEASES.md's Conventional Commits section, referenced from CLAUDE.md's Git workflow); SemVer with an explicit pre-1.0 bump rule; `git-cliff` (`cliff.toml`) generates `CHANGELOG.md`, `cargo-release` drives the version bump as one deliberate, human-run command on `main` — never automatic on push. `release.yml`'s GitHub Release body now comes from that generated CHANGELOG.md section instead of GitHub's auto-generated notes. `ci.yml` gained a guardrail job failing PRs into `main` that don't bump the workspace version. Motivated by a versioning mistake observed in another project (`~/git/wiki-md`): automatic per-push version bumps produced a messy, duplicated release/changelog history instead of curated releases.
