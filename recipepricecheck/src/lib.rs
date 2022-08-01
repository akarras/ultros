use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fmt::{Display, Formatter};
use universalis::{Error as UniversalisError, Error, ListingView, MarketView, UniversalisClient};
use xivapi::models::recipe::{ItemIngredient, Recipe};

#[derive(Debug, Serialize, Deserialize)]
pub struct BestPricingSummary {
    /// Total summary of the pricing
    pub total: i64,
    pub items: Vec<BestPricingForItem>,
}

impl BestPricingSummary {
    pub fn get_items_by_world_cloned(
        &self,
    ) -> BTreeMap<String, Vec<(BestPricingForItem, Vec<ListingView>)>> {
        self.items.iter().map(|m| (m, m.items_by_world())).fold(
            BTreeMap::new(),
            |mut map, (item, item_map)| {
                for (world, listings) in item_map {
                    map.entry(world.clone()).or_default().push((
                        item.clone(),
                        listings.into_iter().map(|m| m.clone()).collect(),
                    ));
                }
                map
            },
        )
    }

    pub fn get_items_by_world(
        &self,
    ) -> BTreeMap<&String, Vec<(&BestPricingForItem, Vec<&ListingView>)>> {
        self.items.iter().map(|m| (m, m.items_by_world())).fold(
            BTreeMap::new(),
            |mut map, (item, item_map)| {
                for (world, listings) in item_map {
                    map.entry(world).or_default().push((item, listings));
                }
                map
            },
        )
    }
}

#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct ItemSummary {
    pub average_price: u32,
    pub lowest_price: u32,
    pub highest_price: u32,
}

impl Display for ItemSummary {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "low: {:>5} avg: {:>5} high: {:>5}",
            self.lowest_price, self.average_price, self.highest_price
        )
    }
}

impl ItemSummary {
    fn from_iter<'a>(item: impl Iterator<Item = &'a ListingView>) -> Option<Self> {
        let (lowest_price, highest_price, acc, count) =
            item.map(|m| m.total / m.quantity.unwrap_or(1)).fold(
                (u32::MAX, u32::MIN, 0, 0),
                |(mut min, mut max, mut acc, mut count), value| {
                    min = min.min(value);
                    max = max.max(value);
                    acc += value;
                    count += 1;
                    (min, max, acc, count)
                },
            );
        if count == 0 {
            return None;
        }
        Some(ItemSummary {
            average_price: acc / count,
            lowest_price,
            highest_price,
        })
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ItemListingsSummary {
    /// Provides a summary of HQ items
    pub hq_items: Option<ItemSummary>,
    /// Provides a summary of LQ items
    pub lq_items: Option<ItemSummary>,
}

impl ItemListingsSummary {
    fn new<'a>(items: impl Iterator<Item = &'a ListingView> + Clone) -> Self {
        Self {
            hq_items: ItemSummary::from_iter(items.clone().filter(|m| m.hq)),
            lq_items: ItemSummary::from_iter(items.filter(|m| !m.hq)),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ListingStatus {
    Good,
    PartialFill,
    Unavailable,
}

fn is_shard(name: &str) -> bool {
    name.contains("Crystal") | name.contains("Shard") | name.contains("Cluster")
}

#[derive(Serialize, Deserialize, Debug, Hash, Eq, PartialEq, PartialOrd, Ord)]
enum Job {
    Alchemist,
    Armoerer,
    Blacksmith,
    Carpenter,
    Culinarian,
    Goldsmith,
    Leatherworker,
    Weaver,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CrafterDetails {
    jobs: HashMap<Job, i32>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PricingArguments {
    /// Checks for whether it is cheaper to craft sub recipes of this item
    /// I.e. if item A requires item B, it will check whether you can craft item B using cheaper items
    pub check_subrecipes: bool,
    /// Useful for check_subrecipes, if the crafter is too low of a level then we won't suggest those recipes
    pub crafter_details: Option<CrafterDetails>,
    /// Homeworld of the crafter. Used to calculate how much margin the crafter makes by doing the craft
    pub crafter_home_world: Option<String>,
    /// Skip buying listings that would leave you with more than 30% of the total quantity you needed
    pub filter_items_with_too_much_quantity: bool,
    /// Whether to include the price of shards in the request
    pub filter_shards: bool,
    pub craft_quantity: i64,
}

impl Default for PricingArguments {
    fn default() -> Self {
        Self {
            check_subrecipes: false,
            crafter_details: None,
            crafter_home_world: None,
            filter_items_with_too_much_quantity: true,
            filter_shards: true,
            craft_quantity: 1,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RecipePricingRawData {
    ingredients: Vec<(i64, ItemIngredient)>,
    recipe_item: i32,
    market_view: MarketView,
}

pub async fn get_ingredient_prices<'a>(
    client: &UniversalisClient,
    world_or_datacenter: &str,
    item: &'a Recipe,
) -> Result<RecipePricingRawData, UniversalisError> {
    let ingredients: Vec<(i64, ItemIngredient)> =
        item.ingredients().map(|(c, i)| (c, i.clone())).collect();
    let mut ids: Vec<_> = ingredients.iter().map(|(_, i)| i.id as i32).collect();
    let recipe_item = item.item_result_target_id as i32;
    ids.push(recipe_item);
    let market_view = client
        .marketboard_current_data(world_or_datacenter, ids.as_slice())
        .await?;
    Ok(RecipePricingRawData {
        ingredients,
        recipe_item,
        market_view,
    })
}

impl RecipePricingRawData {
    pub fn get_recipe_target_pricing_for_world(
        &self,
        world: &str,
    ) -> Result<ItemListingsSummary, Error> {
        let listings = self
            .market_view
            .get_listings_for_item_id(self.recipe_item as u32)?;
        Ok(ItemListingsSummary::new(
            listings
                .into_iter()
                .filter(|m| m.world_name.eq(&Some(world.to_string()))),
        ))
    }

    pub fn get_recipe_target_item_listing_summary(&self) -> Result<ItemListingsSummary, Error> {
        let listings = self
            .market_view
            .get_listings_for_item_id(self.recipe_item as u32)?;
        Ok(ItemListingsSummary::new(listings.into_iter()))
    }

    pub fn create_best_price_summary(
        &self,
        args: &PricingArguments,
    ) -> Result<BestPricingSummary, Error> {
        let number_to_craft = args.craft_quantity;
        let items: Vec<_> = self
            .ingredients
            .iter()
            .filter(|(_, item)| !(args.filter_shards && is_shard(&item.name)))
            .map(|(quantity, ingredient)| {
                let item = self
                    .market_view
                    .get_listings_for_item_id(ingredient.id as u32)
                    .unwrap();
                if item.is_empty() {
                    eprintln!("warning: no listings found for item {}", ingredient.id);
                }
                let mut remaining_quantity = *quantity * number_to_craft;
                let market_ingredients: Vec<_> = item
                    .iter()
                    .sorted_by(|a, b| {
                        a.price_per_unit
                            .unwrap_or(u32::MAX)
                            .cmp(&b.price_per_unit.unwrap_or(u32::MAX))
                    })
                    .filter(|m| m.quantity.is_some())
                    .filter(|m| {
                        if args.filter_items_with_too_much_quantity {
                            let craft_max_stack_size =
                                (*quantity as f64 * number_to_craft as f64 * 1.2) as u32;
                            // keep items with quantity < max stack size
                            m.quantity.unwrap_or_default() < craft_max_stack_size
                        } else {
                            // we're not applying the filter, true allows every item through.
                            true
                        }
                    })
                    .take_while(|m| {
                        let item_needed = remaining_quantity > 0;
                        remaining_quantity -= m.quantity.unwrap_or_default() as i64;
                        item_needed
                    })
                    .cloned()
                    .collect();

                if remaining_quantity < -2 {
                    // TODO test to see if we can remove a listing
                }

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
                    amount_needed: *quantity * number_to_craft,
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
}
