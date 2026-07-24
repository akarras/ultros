//! Page-scoped price zone for `/items/*`.
//!
//! The explorer lets the user pick a world/datacenter/region without
//! touching the global `PRICE_ZONE` cookie: the selection lives in the
//! `?world=` query param and falls back to the cookie (via
//! [`get_price_zone`], which itself folds in the server-guessed region)
//! and finally `"North-America"`.

use leptos::prelude::*;
use leptos::reactive::wrappers::write::{IntoSignalSetter, SignalSetter};
use leptos_router::hooks::query_signal;
use ultros_api_types::world_helper::AnySelector;

use crate::global_state::LocalWorldData;
use crate::global_state::home_world::get_price_zone;

/// Resolved price scope for the item explorer subtree. Provided as
/// context by `ItemExplorer` so the toolbar (picker, chip hrefs) and
/// `ItemList` (item links, world column) agree on one resolution.
#[derive(Clone, Copy)]
pub(crate) struct ExplorerPriceScope {
    /// Canonical name of the active scope (world, datacenter, or region).
    pub name: Signal<String>,
    /// True when the scope is a single world — the "cheapest world"
    /// column is redundant then.
    pub is_single_world: Signal<bool>,
    /// The validated `?world=` query param, if present. Used to carry the
    /// selection across toolbar navigation links; `None` means the URL is
    /// clean and the cookie/guessed default applies.
    pub query_world: Signal<Option<String>>,
    /// Current selection for `WorldPicker`.
    pub picker_value: Signal<Option<AnySelector>>,
    /// Picker setter — writes the query param, never the cookie.
    pub picker_setter: SignalSetter<Option<AnySelector>>,
}

pub(crate) fn use_explorer_price_scope() -> ExplorerPriceScope {
    let (world_q, set_world_q) = query_signal::<String>("world");
    let (price_zone, _) = get_price_zone();
    let worlds = use_context::<LocalWorldData>().and_then(|w| w.0.ok());

    // Unknown names (typos, stale links) fall through to the default
    // rather than producing a scope the API can't serve.
    let worlds_validate = worlds.clone();
    let query_world = Memo::new(move |_| {
        world_q().and_then(|w| {
            worlds_validate
                .as_ref()
                .and_then(|worlds| worlds.lookup_world_by_name(&w))
                .map(|r| r.get_name().to_string())
        })
    });

    let name = Memo::new(move |_| {
        query_world
            .get()
            .or_else(|| price_zone.get().map(|z| z.get_name().to_string()))
            .unwrap_or_else(|| "North-America".to_string())
    });

    let worlds_single = worlds.clone();
    let is_single_world = Memo::new(move |_| {
        name.with(|n| {
            worlds_single
                .as_ref()
                .and_then(|worlds| worlds.lookup_world_by_name(n))
                .map(|r| r.as_world().is_some())
                .unwrap_or(false)
        })
    });

    let worlds_value = worlds.clone();
    let picker_value = Memo::new(move |_| {
        name.with(|n| {
            worlds_value
                .as_ref()
                .and_then(|worlds| worlds.lookup_world_by_name(n))
                .map(|r| AnySelector::from(&r))
        })
    });

    let picker_setter = move |selector: Option<AnySelector>| {
        let name = selector.and_then(|selector| {
            worlds
                .as_ref()
                .and_then(|worlds| worlds.lookup_selector(selector))
                .map(|r| r.get_name().to_string())
        });
        // `None` clears the param, restoring the cookie/guessed default.
        set_world_q(name);
    };

    ExplorerPriceScope {
        name: name.into(),
        is_single_world: is_single_world.into(),
        query_world: query_world.into(),
        picker_value: picker_value.into(),
        picker_setter: picker_setter.into_signal_setter(),
    }
}

/// Append the current `?world=` selection to an explorer-internal href so
/// navigation keeps the picked scope. No-op when the URL has no explicit
/// selection.
pub(crate) fn href_with_world(href: String, world: Option<&str>) -> String {
    match world {
        Some(world) => {
            let encoded: String =
                percent_encoding::utf8_percent_encode(world, percent_encoding::NON_ALPHANUMERIC)
                    .to_string();
            format!("{href}?world={encoded}")
        }
        None => href,
    }
}

#[cfg(test)]
mod tests {
    use super::href_with_world;

    #[test]
    fn href_without_world_is_untouched() {
        assert_eq!(
            href_with_world("/items/category/Sword".to_string(), None),
            "/items/category/Sword",
        );
    }

    #[test]
    fn href_with_world_appends_encoded_query() {
        assert_eq!(
            href_with_world("/items/jobset/PLD".to_string(), Some("Aether")),
            "/items/jobset/PLD?world=Aether",
        );
        // Non-ASCII region names (cn/ko data) must be percent-encoded.
        assert_eq!(
            href_with_world("/items/jobset/PLD".to_string(), Some("中国")),
            "/items/jobset/PLD?world=%E4%B8%AD%E5%9B%BD",
        );
    }
}
