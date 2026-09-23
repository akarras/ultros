//! Collectables turn-ins and the scrip each one pays.

use std::collections::HashSet;

use xiv_gen::CollectablesShopRewardScripId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScripType {
    OrangeCrafters,
    OrangeGatherers,
    WhiteCrafters,
    PurpleCrafters,
    WhiteGatherers,
    PurpleGatherers,
    Other(u32),
}

impl ScripType {
    /// Map a `CollectablesShopRewardScrip.Currency` value to the scrip it pays.
    ///
    /// `Currency` is a small **enum index**, not an item id — every row in the
    /// 7.55 data carries `0`, `2`, `4`, `6` or `7`. Matching it against scrip
    /// item ids is what left this page blank, so the mapping below is derived
    /// from the game data instead. Joining `CollectablesShopItem` to the
    /// `RewardType = 1` (scrip-paying) shops, ignoring the material-exchange
    /// shops that reuse this column, gives:
    ///
    /// | Currency | rows | turn-ins |
    /// |---|---|---|
    /// | 2 | 1089 | crafted, lv 50-99 |
    /// | 4 |  163 | gathered/fished, lv 50-98 |
    /// | 6 |   93 | crafted, lv 78-80 and lv 100 |
    /// | 7 |   18 | gathered/fished, lv 100 |
    ///
    /// So `2`/`4` are the purple (levelling) crafter/gatherer pair and `6`/`7`
    /// the orange (level 100) pair. Currency `6`'s level-100 rows are exactly
    /// one item per crafting job — the eight "Rarefied" max-level crafts — which
    /// is what pins it to Orange Crafters' rather than the retired white scrip;
    /// its lv 78-80 rows are the Shadowbringers tier that collapsed into the
    /// same high-tier crafter slot when white scrips were removed in 7.0.
    pub fn from_currency(currency: u32) -> Self {
        match currency {
            2 => ScripType::PurpleCrafters,
            4 => ScripType::PurpleGatherers,
            6 => ScripType::OrangeCrafters,
            7 => ScripType::OrangeGatherers,
            other => ScripType::Other(other),
        }
    }

    /// The `?scrip=` query value that selects this type, as emitted by the
    /// toolbar `<select>`.
    pub fn from_filter_key(key: &str) -> Option<Self> {
        match key {
            "OrangeCrafters" => Some(ScripType::OrangeCrafters),
            "OrangeGatherers" => Some(ScripType::OrangeGatherers),
            "WhiteCrafters" => Some(ScripType::WhiteCrafters),
            "PurpleCrafters" => Some(ScripType::PurpleCrafters),
            "WhiteGatherers" => Some(ScripType::WhiteGatherers),
            "PurpleGatherers" => Some(ScripType::PurpleGatherers),
            _ => None,
        }
    }

    /// Inverse of [`from_filter_key`](Self::from_filter_key): the `?scrip=`
    /// value that selects this type, or `None` for a currency index the page
    /// doesn't name.
    pub fn filter_key(self) -> Option<&'static str> {
        Some(match self {
            ScripType::OrangeCrafters => "OrangeCrafters",
            ScripType::OrangeGatherers => "OrangeGatherers",
            ScripType::WhiteCrafters => "WhiteCrafters",
            ScripType::PurpleCrafters => "PurpleCrafters",
            ScripType::WhiteGatherers => "WhiteGatherers",
            ScripType::PurpleGatherers => "PurpleGatherers",
            ScripType::Other(_) => return None,
        })
    }

    /// English item name of the scrip currency itself, for looking the item
    /// up in the (English) game-data pack. The white scrips were retired in
    /// 7.0 and have no item.
    pub fn item_name(self) -> Option<&'static str> {
        Some(match self {
            ScripType::OrangeCrafters => "Orange Crafters' Scrip",
            ScripType::OrangeGatherers => "Orange Gatherers' Scrip",
            ScripType::PurpleCrafters => "Purple Crafters' Scrip",
            ScripType::PurpleGatherers => "Purple Gatherers' Scrip",
            ScripType::WhiteCrafters | ScripType::WhiteGatherers | ScripType::Other(_) => {
                return None;
            }
        })
    }

    /// Gatherer scrips are paid for collectables that are *gathered*, not
    /// crafted, so the craft-cost model below can never price them. The page
    /// keeps the options selectable but explains the empty table instead of
    /// silently rendering nothing.
    pub fn is_gatherer(&self) -> bool {
        matches!(
            self,
            ScripType::OrangeGatherers | ScripType::WhiteGatherers | ScripType::PurpleGatherers
        )
    }
}

/// A single collectables turn-in: the item handed in, the scrip it pays and how
/// much it pays at maximum collectability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScripTurnIn {
    pub item_id: i32,
    pub scrip_type: ScripType,
    pub scrip_amount: u32,
}

/// `CollectablesShop.RewardType` for the turn-in counters that pay scrip.
const SCRIP_REWARD_TYPE: i32 = 1;
/// `CollectablesShop.RewardType` for the material exchanges, which hand back
/// items and pay no scrip.
const MATERIAL_EXCHANGE_REWARD_TYPE: i32 = 2;

/// `CollectablesShopItem` groups that belong *only* to material-exchange shops.
///
/// `CollectablesShop.ShopItems[..]` lists a shop's item groups, and a group is
/// the integer half of `CollectablesShopItem`'s `<group>.<index>` key — which is
/// how `collectables_shop_items` is keyed, so the two join directly.
///
/// This deliberately collects the groups to *exclude* rather than the ones to
/// keep. A group nobody claims, an unknown future `RewardType`, or a renamed
/// sheet that leaves `collectables_shops` empty then all degrade to today's
/// behaviour — a few oddly-labelled rows — instead of blanking the page, which
/// is the failure mode this route has already shipped once. A group claimed by
/// a scrip shop *and* an exchange shop stays visible for the same reason.
fn material_exchange_groups(data: &xiv_gen::Data) -> HashSet<i32> {
    let mut scrip_paying = HashSet::new();
    let mut exchange_only = HashSet::new();

    for shop in data.collectables_shops.values() {
        let bucket = match shop.reward_type {
            SCRIP_REWARD_TYPE => &mut scrip_paying,
            MATERIAL_EXCHANGE_REWARD_TYPE => &mut exchange_only,
            _ => continue,
        };
        for group in shop.shop_items {
            if group != 0 {
                bucket.insert(group);
            }
        }
    }

    exchange_only.retain(|group| !scrip_paying.contains(group));
    exchange_only
}

/// Every turn-in the collectables shops offer, before any UI filtering or
/// pricing.
///
/// Material-exchange trades are dropped here: they populate the same
/// `CollectablesShopRewardScrip.Currency` column the real turn-ins do, so
/// reading that column alone lists every one of them as a scrip source paying a
/// scrip it never awards.
pub fn scrip_turn_ins(data: &xiv_gen::Data) -> Vec<ScripTurnIn> {
    let exchange_only = material_exchange_groups(data);
    let mut turn_ins = Vec::new();

    for (group, item_vec) in &data.collectables_shop_items {
        if exchange_only.contains(&group.0) {
            continue;
        }
        for item_entry in item_vec {
            let reward_scrip_id = item_entry.collectables_shop_reward_scrip;
            if reward_scrip_id == 0 {
                continue;
            }

            let reward = match data
                .collectables_shop_reward_scrips
                .get(&CollectablesShopRewardScripId(reward_scrip_id))
            {
                Some(r) => r,
                None => continue,
            };

            let scrip_amount = reward.high_reward as u32;
            if scrip_amount == 0 {
                continue;
            }

            turn_ins.push(ScripTurnIn {
                item_id: item_entry.item,
                scrip_type: ScripType::from_currency(reward.currency as u32),
                scrip_amount,
            });
        }
    }

    turn_ins
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `Currency` value that actually occurs in `CollectablesShopRewardScrip`
    /// (7.55: `0`, `2`, `4`, `6`, `7` — `0` being the null row, which is already
    /// dropped for having a zero reward). If any of these falls through to
    /// `Other`, every row awarding it disappears from the page.
    #[test]
    fn every_live_currency_value_is_recognised() {
        for currency in [2, 4, 6, 7] {
            assert!(
                !matches!(ScripType::from_currency(currency), ScripType::Other(_)),
                "currency {currency} is unmapped, so its rows never render"
            );
        }
    }

    /// `CollectablesShopRewardScrip.Currency` is a small **enum index**, not an
    /// item id: `2`/`4` are the purple crafter/gatherer pair paid by lv 50-99
    /// turn-ins, `6`/`7` the orange pair paid at level 100.
    #[test]
    fn currency_indices_map_to_the_right_scrip() {
        assert_eq!(ScripType::from_currency(2), ScripType::PurpleCrafters);
        assert_eq!(ScripType::from_currency(4), ScripType::PurpleGatherers);
        assert_eq!(ScripType::from_currency(6), ScripType::OrangeCrafters);
        assert_eq!(ScripType::from_currency(7), ScripType::OrangeGatherers);
    }

    /// The bug this replaced: `from_currency` was fed `reward.currency` but
    /// matched on scrip **item** ids, so no real currency value ever matched and
    /// the whole page rendered zero rows. Item ids must not be accepted here.
    #[test]
    fn scrip_item_ids_are_not_currency_values() {
        for item_id in [41784, 41785, 25199, 33913, 25200, 33914] {
            assert_eq!(
                ScripType::from_currency(item_id),
                ScripType::Other(item_id),
                "item id {item_id} was treated as a currency index"
            );
        }
    }

    /// The material exchanges (`CollectablesShop.RewardType == 2`) hand back
    /// *items*, not scrip — but they populate the same
    /// `CollectablesShopRewardScrip.Currency` column the turn-in counters do, so
    /// reading that column without joining `RewardType` lists every one of their
    /// trades as a scrip source paying a scrip it never awards.
    ///
    /// These four are the craftable head of each `RewardType == 2` shop on the
    /// pinned 7.55 data; ids are used rather than names because `Item.name` is
    /// per-locale.
    #[test]
    fn material_exchange_trades_are_not_scrip_turn_ins() {
        let data = xiv_gen_db::data();
        let turn_ins = scrip_turn_ins(data);

        for (item_id, shop) in [
            (31101, "Oddly Specific Materials Exchange (Crafting)"),
            (31750, "Oddly Delicate Materials Exchange"),
            (36311, "Resplendent Materials Exchange"),
            (38756, "Trade Goods Exchange"),
        ] {
            assert!(
                !turn_ins.iter().any(|t| t.item_id == item_id),
                "item {item_id} is traded at the {shop}, which pays no scrip, \
                 but it is listed as a scrip turn-in"
            );
        }
    }

    /// Excluding the material exchanges must not empty the page — this route has
    /// already shipped once rendering zero rows, and a join that silently
    /// matches nothing would put it straight back there.
    #[test]
    fn the_real_turn_in_counters_survive_the_exclusion() {
        let data = xiv_gen_db::data();
        let turn_ins = scrip_turn_ins(data);

        assert!(
            turn_ins.len() > 1000,
            "only {} turn-ins survived; the RewardType join has stopped matching",
            turn_ins.len()
        );
        // A Dwarven collectable handed in for Orange Crafters' Scrip.
        assert!(
            turn_ins.iter().any(|t| t.item_id == 26271),
            "a known scrip turn-in was excluded along with the material exchanges"
        );
    }

    /// The exclusion set has to be non-empty, and must never swallow a group
    /// that a scrip-paying shop offers.
    #[test]
    fn only_material_exchange_groups_are_excluded() {
        let data = xiv_gen_db::data();
        let excluded = material_exchange_groups(data);

        assert!(
            !excluded.is_empty(),
            "no material-exchange groups found; CollectablesShop did not load"
        );
        for shop in data.collectables_shops.values() {
            if shop.reward_type != SCRIP_REWARD_TYPE {
                continue;
            }
            for group in shop.shop_items {
                assert!(
                    group == 0 || !excluded.contains(&group),
                    "group {group} pays scrip but was excluded"
                );
            }
        }
    }

    /// Gatherer scrips are paid for collectables that are *gathered*, so no
    /// turn-in awarding one can have a recipe. The page relies on this: it
    /// prices craft costs, skips anything without a recipe, and tells the user
    /// the gatherer filters are empty by design instead of rendering a blank
    /// table.
    ///
    /// Before the `RewardType` join this was false — 59 craftable material
    /// exchange trades carried `Currency = 4`, so `?scrip=PurpleGatherers`
    /// rendered 59 rows, every one of them wrong.
    #[test]
    fn no_craftable_turn_in_pays_a_gatherer_scrip() {
        let data = xiv_gen_db::data();
        let mut craftable = std::collections::HashSet::new();
        for recipe in data.recipes.values() {
            craftable.insert(recipe.item_result);
        }

        let offenders: Vec<i32> = scrip_turn_ins(data)
            .into_iter()
            .filter(|t| t.scrip_type.is_gatherer() && craftable.contains(&t.item_id))
            .map(|t| t.item_id)
            .collect();

        assert!(
            offenders.is_empty(),
            "{} craftable turn-ins are labelled a gatherer scrip, so the \
             gatherer filters render rows the page says can never exist: {:?}",
            offenders.len(),
            &offenders[..offenders.len().min(8)]
        );
    }
}
