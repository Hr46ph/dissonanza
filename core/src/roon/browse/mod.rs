//! `com.roonlabs.browse:1` — Roon's hierarchical navigation used for sidebar categories, browse
//! paths, and search. Unlike `connection`, this is a service the Core provides and this extension
//! consumes: declared in `connection`'s `REQUIRED_SERVICES` at registration, then driven entirely
//! through `connection::ConnectionRequests` (the same seam `transport` uses) — this module never
//! touches SOOD discovery, pairing, or reconnect itself, per CLAUDE.md §1.
//!
//! Unlike `transport:2`, there is no subscription/`CONTINUE` stream anywhere in this service: both
//! `browse` and `load` are one-shot request/response calls, and the browse stack itself lives
//! entirely on the Core (keyed by `hierarchy`/`multi_session_key`) — this module holds no session
//! state of its own, per docs/protocol/browse.md's "Session/browse-stack semantics" and
//! IMPL_BROWSE.md's resolved architectural question.
//!
//! Left as an acknowledged, unconfirmed gap: no source studied for docs/protocol/browse.md states
//! whether a Core-side browse-stack position survives a `connection` reconnect.

mod error;
mod model;
mod request;

pub use error::BrowseError;
pub use model::{InputPrompt, Item, ItemHint, List, ListHint};
pub use request::{
    BrowseAction, BrowseOptions, BrowseResult, Hierarchy, LoadOptions, LoadResult, browse, load,
};
