# RELEASES.md

How a release actually gets cut, built, and published, and what still needs manual setup before each piece works end-to-end. Ties together the branch model in [CLAUDE.md](CLAUDE.md)'s Git workflow section, the packaging choices in [TECH_STACK.md](TECH_STACK.md), and the CI/packaging files listed below.

## Versioning

SemVer (`MAJOR.MINOR.PATCH`). Every release is a git tag on `main` (`v0.1.0`, `v0.2.0`, ...) — everything downstream (binaries, the Flatpak manifest, the AUR `PKGBUILD`) is built from that tag, never from a floating branch.

## Release flow

1. Feature/phase branches merge into `develop` as work completes (see CLAUDE.md's Git workflow).
2. When `develop` is release-ready, merge `develop` → `main`.
3. Tag `main` with `vX.Y.Z` and push the tag. This triggers `.github/workflows/release.yml`.
4. Write release notes (a short "what changed" — a full `CHANGELOG.md` can come later if the notes outgrow a GitHub Release body).

## What CI does automatically vs. what's still manual

| Step | Automated? | Where |
|---|---|---|
| Format/lint/test on every push/PR | Yes | `.github/workflows/ci.yml` |
| Build release binary + attach to GitHub Release on tag push | Yes | `.github/workflows/release.yml` |
| Validate the Flatpak manifest builds | Yes (in CI, doesn't publish) | `.github/workflows/release.yml` |
| Submit to Flathub (first time) | **No — manual, one-time** | see below |
| Update the Flathub manifest for new releases | Partially — bumping the tag is a small manual PR to a separate Flathub-managed repo, sometimes bot-assisted | see below |
| Push `PKGBUILD` update to AUR | Scaffolded, needs secrets configured (see below) | `.github/workflows/release.yml` |

## Flatpak / Flathub

- Manifest: `packaging/flatpak/io.github.hr46ph.Dissonanza.yml`. App ID follows the `io.github.<username>.<AppName>` convention for GitHub-hosted apps (avoids using "Roon" in the ID for trademark reasons).
- **Blocker**: Flathub builds from your repo, so the repo must be **public** before a Flathub submission is possible. TECH_STACK.md currently has this as "private repo for now" — revisit that decision before attempting the Flathub PR.
- **Blocker**: Flathub builds happen with no network access. Cargo dependencies must be vendored via a generated `cargo-sources.json` (using `flatpak-builder-tools`'s `flatpak-cargo-generator.py` against `Cargo.lock`). This doesn't exist yet because there's no `Cargo.lock` yet — generate it once the workspace has real dependencies.
- First submission is a one-time PR to the `flathub/flathub` GitHub repo adding the manifest. After a reviewer approves, Flathub creates a dedicated repo for the app; subsequent updates are small PRs to that repo bumping the source tag/commit.

## AUR

- `packaging/aur/PKGBUILD`. Pushed to a personal git repo at `aur.archlinux.org` — no review process, you're the maintainer.
- **First push is manual** (claiming the package name). After that, `release.yml`'s AUR job can push version bumps automatically, but only once `AUR_SSH_PRIVATE_KEY` (and matching AUR account SSH key) is added as a repo secret — the job is written to skip quietly until that secret exists.
- `sha256sums` in the `PKGBUILD` are placeholders (`SKIP`) until a real release tag exists to compute them against.

## Known gaps to close before the first real release

- Repo visibility (private → public) before any Flathub submission.
- `cargo-sources.json` generation, once dependencies are locked in.
- Flatpak system dependencies / SDK extensions in the manifest are a best-guess for a Slint app — verify once the app actually builds.
- AUR secrets not yet configured (job present but inert).
- No code exists yet (see CURRENT_STATE.md) — `ci.yml` will fail until the Cargo workspace exists. That's expected, not a bug in the workflow.
