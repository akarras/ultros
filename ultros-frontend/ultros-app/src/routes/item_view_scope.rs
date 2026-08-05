//! URL construction for the item view.

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

/// Characters left untouched by JS `encodeURIComponent` — the function
/// backing [`leptos_router::location::Url::escape`] on the client (its `ssr`
/// build instead percent-encodes every non-alphanumeric byte, `-` included).
/// Encoding against that wider client-side unreserved set here keeps the
/// server-rendered href identical to what the client would produce, so a
/// hyphenated scope name like `"North-America"` doesn't trip a hydration
/// mismatch — the same class of bug documented at the top of `item_view.rs`.
const COMPONENT_UNRESERVED: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

/// Canonical item URL for a scope name, carrying the current query string
/// forward.
///
/// Switching worlds must not discard the reader's filters: `?exclude-worlds=`,
/// `?compare-buy-from=`, and `?lens=` once the lens work lands.
pub fn item_href(world: &str, item_id: i32, query: &str) -> String {
    let escaped_world = utf8_percent_encode(world, COMPONENT_UNRESERVED).to_string();
    let path = format!("/item/{escaped_world}/{item_id}");
    if query.is_empty() {
        path
    } else {
        format!("{path}?{query}")
    }
}

/// Query param naming the buy world for the item page's flip-verification card.
#[allow(dead_code)]
pub const COMPARE_BUY_FROM_PARAM: &str = "compare-buy-from";

/// Item URL that opens the flip-verification card: sell world in the path,
/// buy world in `?compare-buy-from=`. An unresolvable (empty) buy world
/// degrades to the plain item link.
#[allow(dead_code)]
pub fn compare_item_href(sell_world: &str, item_id: i32, buy_world: &str) -> String {
    if buy_world.is_empty() {
        return item_href(sell_world, item_id, "");
    }
    let escaped_buy = utf8_percent_encode(buy_world, COMPONENT_UNRESERVED).to_string();
    item_href(
        sell_world,
        item_id,
        &format!("{COMPARE_BUY_FROM_PARAM}={escaped_buy}"),
    )
}

#[cfg(test)]
mod tests {
    use super::{compare_item_href, item_href};

    #[test]
    fn empty_query_yields_a_clean_path() {
        assert_eq!(item_href("Aether", 40644, ""), "/item/Aether/40644");
    }

    #[test]
    fn query_is_appended() {
        assert_eq!(
            item_href("Aether", 40644, "exclude-worlds=100,200"),
            "/item/Aether/40644?exclude-worlds=100,200",
        );
    }

    #[test]
    fn plain_world_names_are_unchanged() {
        assert_eq!(item_href("North-America", 1, ""), "/item/North-America/1");
    }

    #[test]
    fn compare_href_carries_buy_world_param() {
        assert_eq!(
            compare_item_href("Gilgamesh", 40644, "Jenova"),
            "/item/Gilgamesh/40644?compare-buy-from=Jenova",
        );
    }

    #[test]
    fn compare_href_encodes_hyphenated_names_stably() {
        // Hyphens survive both sides (hydration-mismatch guard, same as item_href).
        assert_eq!(
            compare_item_href("North-America", 1, "Ravana"),
            "/item/North-America/1?compare-buy-from=Ravana",
        );
    }

    #[test]
    fn compare_href_without_buy_world_is_a_plain_item_link() {
        assert_eq!(
            compare_item_href("Gilgamesh", 40644, ""),
            "/item/Gilgamesh/40644"
        );
    }

    #[test]
    fn item_href_carries_compare_param_across_world_switches() {
        assert_eq!(
            item_href("Sargatanas", 40644, "compare-buy-from=Jenova"),
            "/item/Sargatanas/40644?compare-buy-from=Jenova",
        );
    }
}
