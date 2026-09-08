# Dissonanza

A native Linux remote client for [Roon](https://roon.app), built entirely on Roon's officially
supported API.

**Musical meaning** — *dissonanza* or dissonance refers to tension between notes, perceived as unstable relative to consonance, that may or may not resolve depending on style or intent.

I picked this name because it carries symbolism in the absence of a native Linux client despite there being a demand for one.

For those interrested, here are a few examples of classical and popular pieces that use dissonance deliberately.

| # | Composer | Work | How dissonance is used |
|---|---|---|---|
| 1 | Mozart | String Quartet No. 19, K. 465 — “Dissonance” | Deliberate harmonic tension and delayed resolution in the introduction |
| 2 | Wagner | Tristan und Isolde — Prelude | Prolonged, unresolved tension illustrated by the Tristan chord |
| 3 | Stravinsky | The Rite of Spring | Aggressive clashes of harmonies create intense tension and instability |
| 4 | Beethoven | Symphony No. 9 — Finale | Dissonance is used as dramatic tension leading toward harmonic resolution |
| 5 | Schoenberg | Verklärte Nacht / later atonal works | Dissonance increasingly becomes independent and no longer necessarily requires resolution |

| # | Artist | Work | How dissonance is used |
|---|---|---|---|
| 1 | The Beatles | A Day in the Life | An orchestral crescendo creates extreme tension and an almost surreal effect |
| 2 | Opeth | The Drapery Falls | An atonal passage built on alternating melodic tritones creates a dissociative, unresolved effect |
| 3 | Black Sabbath | Black Sabbath | The tritone is deliberately used to create a dark and ominous atmosphere |
| 4 | Elvis Costello | I Want You | A repeated two-note dissonant guitar solo underscores the song's theme of jealous obsession |
| 5 | Radiohead | Paranoid Android | Unexpected harmonies and dissonant relationships create continuous tension |


## Status

Early development, not yet usable. The Roon connection layer (SOOD discovery, MOO protocol) is
in progress; the GUI hasn't started yet.

The project is experimental with the main purpose of learning the development process with Rust. It might never finish or even see a release.

## Project layout

Cargo workspace with two crates:

- `core/` — Roon SOOD/MOO protocol client and all business logic, no GUI dependency.
- `app/` — the [Slint](https://slint.dev) GUI shell, depends only on `core`'s public API.

## Building

```sh
cargo build
```

Requires a recent stable Rust toolchain (2024 edition).

## License

MIT — see [LICENSE](LICENSE).
