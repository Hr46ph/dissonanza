# NORTH-STAR.md

Where this project is ultimately headed. Used when a judgment call has no clear precedent elsewhere — align the decision with the long-term direction described here rather than optimizing only for the immediate task.

## Vision

A full-featured, native Linux remote client for Roon that matches the official remote's capabilities and visual polish, while adding what Roon itself doesn't offer (deeper zoom/browse) — built entirely on Roon's officially supported API, never on reverse-engineered internals.

## Non-goals

- Reverse-engineering Roon's internal protocol to reach Settings, Audio Setup, Zone config, or the DSP engine — this stays permanently out of scope, not just deferred for v1.
- Multi-Core or multi-zone support — controlling one Core at a time is a deliberate, permanent design choice, not a temporary limitation.
- Being an audio player or streaming client — this is control-plane only; the API can't expose or inject the audio stream itself (that runs over Roon's proprietary RAAT protocol).
- Being a scrobbling/Last.fm app in its own right — scrobbling (play-history submission) is auxiliary, a possible future bonus milestone, not a core focus. Track love/unlove isn't part of this non-goal at all: it's Roon's own native control (which Roon already syncs to Last.fm on its own), so surfacing it is ordinary feature parity, not a Last.fm integration this app builds.

## Success looks like

Feature parity with the official Roon remote, period — every sidebar category, browse path, and playback control it offers, matched. That baseline includes surfacing Roon's own track love/unlove control — it's Roon's feature, not an extra. The extras (zoom, Last.fm scrobbling) are bonus milestones on top of that baseline, not a substitute for it.
