use itertools::Itertools;
use std::collections::HashMap;
use std::ops::AddAssign;
use universalis::{Error as UniversalisError, ListingView};
use xivapi::models::recipe::{ItemIngredient, Recipe};

#[derive(Debug)]
pub struct BestPricingSummary {
    /// Total summary of the pricing
    pub total: i64,
    pub items: Vec<BestPricingForItem>,
}

#[derive(Debug)]
pub struct BestPricingForItem {
    pub name: String,
    pub item: u32,
    pub amount_needed: i64,
    pub market_ingredients: Vec<ListingView>,
    pub listing_status: ListingStatus,
}

impl BestPricingForItem {
    pub fn items_by_world(&self) -> HashMap<&String, Vec<&ListingView>> {
        self.market_ingredients
            .iter()
            .into_group_map_by(|e| e.world_name.as_ref().unwrap())
    }
}

#[derive(Debug)]
pub enum ListingStatus {
    Good,
    PartialFill,
    Unavailable,
}

fn is_shard(name: &str) -> bool {
    name.contains("Crystal") | name.contains("Shard") | name.contains("Cluster")
}


pub async fn best_pricing(
    world_or_datacenter: &str,
    item: Recipe,
    recipe_count: i64,
    filter_shards: bool,
) -> Result<BestPricingSummary, UniversalisError> {
    let non_shard_ingredients: Vec<(i64, &ItemIngredient)> = item
        .ingredients()
        .filter(|(_, item)| filter_shards && !is_shard(&item.name))
        .collect();
    let client = universalis::UniversalisClient::new();
    let ids: Vec<_> = non_shard_ingredients
        .iter()
        .map(|(_, i)| i.id as i32)
        .collect();
    let market_view = client
        .marketboard_current_data(world_or_datacenter, ids.as_slice())
        .await?;
    let items: Vec<_> = non_shard_ingredients
        .iter()
        .map(|(quantity, ingredient)| {
            let item = market_view
                .get_listings_for_item_id(ingredient.id as u32)
                .unwrap();
            if item.is_empty() {
                eprintln!("warning: no listings found for item {}", ingredient.id);
            }
            let mut remaining_quantity = *quantity * recipe_count;
            let market_ingredients: Vec<_> = item
                .iter()
                .sorted_by(|a, b| {
                    a.price_per_unit
                        .unwrap_or(u32::MAX)
                        .cmp(&b.price_per_unit.unwrap_or(u32::MAX))
                })
                .filter(|m| m.quantity.is_some())
                .take_while(|m| {
                    let item_needed = remaining_quantity > 0;
                    remaining_quantity -= m.quantity.unwrap_or_default() as i64;
                    item_needed
                })
                .cloned()
                .collect();

            let listing_status = if remaining_quantity == *quantity {
                ListingStatus::PartialFill
            } else if remaining_quantity > 0 {
                ListingStatus::Unavailable
            } else {
                ListingStatus::Good
            };
            BestPricingForItem {
                name: ingredient.name.clone(),
                item: ingredient.id as u32,
                amount_needed: *quantity * recipe_count,
                market_ingredients,
                listing_status,
            }
        })
        .collect();
    let total = items
        .iter()
        .map(|m| m.market_ingredients.iter().map(|m| m.total).sum::<u32>())
        .sum::<u32>() as i64;
    Ok(BestPricingSummary { total, items })
}
