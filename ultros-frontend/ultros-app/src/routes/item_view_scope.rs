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
/// Switching worlds must not discard the reader's filters: `?exclude-worlds=`
/// today, and `?lens=` once the lens work lands.
pub fn item_href(world: &str, item_id: i32, query: &str) -> String {
    let escaped_world = utf8_percent_encode(world, COMPONENT_UNRESERVED).to_string();
    let path = format!("/item/{escaped_world}/{item_id}");
    if query.is_empty() {
        path
    } else {
        format!("{path}?{query}")
    }
}

#[cfg(test)]
mod tests {
    use super::item_href;

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
}
