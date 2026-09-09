//! Crafting calculations and app-owned vendor-data defaults.

pub use ultros_crafting::cost::*;

use std::{collections::HashMap, sync::OnceLock};

/// item_id -> NPC gil-shop unit price, for every gil-shop-sold item.
/// Built once per process from game data (same construction as the
/// vendor-resale page).
pub fn vendor_price_map() -> &'static HashMap<i32, i32> {
    static MAP: OnceLock<HashMap<i32, i32>> = OnceLock::new();
    MAP.get_or_init(|| {
        let data = crate::global_state::xiv_data::tracked_data();
        let mut map = HashMap::new();
        for items in data.gil_shop_items.values() {
            for shop_item in items {
                if let Some(item) = data.items.get(&xiv_gen::ItemId(shop_item.item))
                    && item.price_mid > 0
                {
                    map.insert(shop_item.item, item.price_mid as i32);
                }
            }
        }
        map
    })
}

/// Defaults that match the existing item-page behavior (no subcrafts,
/// no HQ preference, no on-hand) plus the new ExcludeShards default.
#[allow(dead_code)]
pub fn item_page_default(on_hand: &dyn OnHand) -> CraftingCostOptions<'_> {
    CraftingCostOptions {
        require_hq: false,
        max_subcraft_depth: 0,
        shards: ShardsMode::ExcludeShards,
        on_hand,
        vendor_prices: Some(vendor_price_map()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_page_default_options_match_existing_behavior() {
        let oh = EmptyOnHand;
        let opts = item_page_default(&oh);
        assert!(!opts.require_hq);
        assert_eq!(opts.max_subcraft_depth, 0);
        assert_eq!(opts.shards, ShardsMode::ExcludeShards);
    }
}
