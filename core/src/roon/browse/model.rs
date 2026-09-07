//! Typed `com.roonlabs.browse:1` data model — `List`, `Item`, and `InputPrompt` — per
//! `docs/protocol/browse.md`'s "Data model" section. Pure `serde::Deserialize` types, no I/O;
//! parsed out of `browse`/`load` response bodies by `super::request`.

use serde::Deserialize;

/// An unrecognized hint value parses into `Other` rather than a hard error, per the JSDoc's own
/// forward-compatibility instruction ("if you see a hint you do not recognize, treat it as
/// `null`") — mirroring `transport::model::VolumeType::Other`'s precedent. A caller should treat
/// `None` (absent, or JSON `null`) and `Some(Other)` (an unrecognized string) the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListHint {
    ActionList,
    #[serde(other)]
    Other,
}

/// See [`ListHint`]'s forward-compatibility note — the same applies here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemHint {
    Action,
    ActionList,
    List,
    Header,
    #[serde(other)]
    Other,
}

/// One level of the browse stack. The Core owns the actual stack (see
/// docs/protocol/browse.md's "Session/browse-stack semantics") — this is just the current level's
/// display data, returned by `browse`'s `"list"` action or by `load`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct List {
    pub title: String,
    pub count: u64,
    #[serde(default)]
    pub subtitle: Option<String>,
    /// Opaque key, resolved through `image:1` — out of scope for this module, same boundary
    /// `transport::model`'s `image_key` fields already draw.
    #[serde(default)]
    pub image_key: Option<String>,
    pub level: u64,
    #[serde(default)]
    pub display_offset: Option<u64>,
    #[serde(default)]
    pub hint: Option<ListHint>,
}

/// One entry within a [`List`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Item {
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub image_key: Option<String>,
    /// Pass this into the next `browse` call when the user selects this item.
    #[serde(default)]
    pub item_key: Option<String>,
    #[serde(default)]
    pub hint: Option<ItemHint>,
    /// Present only when selecting this item requires free-text input first (e.g. a search box).
    #[serde(default)]
    pub input_prompt: Option<InputPrompt>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct InputPrompt {
    /// The label to show the user, e.g. "Search Albums".
    pub prompt: String,
    /// The button verb that goes with this input, e.g. "Go".
    pub action: String,
    /// Pre-populates the input field if present.
    #[serde(default)]
    pub value: Option<String>,
    pub is_password: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_fully_populated_list() {
        let value = serde_json::json!({
            "title": "Albums",
            "count": 42,
            "subtitle": "By artist",
            "image_key": "img-1",
            "level": 2,
            "display_offset": 10,
            "hint": "action_list",
        });

        let list: List = serde_json::from_value(value).expect("parses");

        assert_eq!(list.title, "Albums");
        assert_eq!(list.count, 42);
        assert_eq!(list.hint, Some(ListHint::ActionList));
    }

    #[test]
    fn parses_a_minimal_list_with_no_optional_fields() {
        let value = serde_json::json!({ "title": "Root", "count": 0, "level": 0 });

        let list: List = serde_json::from_value(value).expect("parses");

        assert_eq!(list.subtitle, None);
        assert_eq!(list.hint, None);
    }

    #[test]
    fn an_unrecognized_list_hint_falls_back_to_other() {
        let value = serde_json::json!({
            "title": "Root",
            "count": 0,
            "level": 0,
            "hint": "some_future_hint",
        });

        let list: List = serde_json::from_value(value).expect("parses");

        assert_eq!(list.hint, Some(ListHint::Other));
    }

    #[test]
    fn a_null_list_hint_parses_as_none() {
        let value = serde_json::json!({
            "title": "Root",
            "count": 0,
            "level": 0,
            "hint": null,
        });

        let list: List = serde_json::from_value(value).expect("parses");

        assert_eq!(list.hint, None);
    }

    #[test]
    fn parses_an_item_with_input_prompt() {
        let value = serde_json::json!({
            "title": "Search",
            "hint": "action",
            "input_prompt": {
                "prompt": "Search Albums",
                "action": "Go",
                "value": "previous query",
                "is_password": false,
            },
        });

        let item: Item = serde_json::from_value(value).expect("parses");

        assert_eq!(item.hint, Some(ItemHint::Action));
        let prompt = item.input_prompt.expect("input_prompt present");
        assert_eq!(prompt.prompt, "Search Albums");
        assert_eq!(prompt.value.as_deref(), Some("previous query"));
    }

    #[test]
    fn an_unrecognized_item_hint_falls_back_to_other() {
        let value = serde_json::json!({ "title": "Thing", "hint": "some_future_hint" });

        let item: Item = serde_json::from_value(value).expect("parses");

        assert_eq!(item.hint, Some(ItemHint::Other));
    }

    #[test]
    fn ignores_unknown_fields() {
        let value = serde_json::json!({
            "title": "Root",
            "count": 0,
            "level": 0,
            "extra_unknown_field": "ignored",
        });

        serde_json::from_value::<List>(value).expect("unknown fields don't break parsing");
    }
}
