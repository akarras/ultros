use clap::Parser;
use itertools::Itertools;
use num_format::{Locale, ToFormattedString};
use recipepricecheck::{best_pricing, BestPricingForItem, ListingStatus};
use std::collections::HashMap;
use universalis::ListingView;
use xivapi::{RecipeRequest, XivDataType};

#[derive(Debug, Parser)]
#[clap(author, version, about)]
struct Args {
    #[clap(short, long, value_parser)]
    recipe_name: String,
    #[clap(short, long, value_parser)]
    world_name: String,
    #[clap(short, long, value_parser)]
    quantity: i64,
    #[clap(short, long, value_parser, default_value="true")]
    filter_shards: bool,
}

#[tokio::main]
async fn main() {
    let args: Args = Args::parse();
    // find the recipe first
    let index = xivapi::get_index(&XivDataType::Recipe);
    let search = index
        .search(&args.recipe_name)
        .next()
        .expect(&format!("No recipes found {}", args.recipe_name));
    let recipe = xivapi::disk_query(RecipeRequest::new(search.id as u32))
        .await
        .unwrap();
    let best_pricing = best_pricing(&args.world_name, recipe, args.quantity, args.filter_shards)
        .await
        .unwrap();
    let world_to_item_map: HashMap<&String, Vec<(&BestPricingForItem, Vec<&ListingView>)>> =
        best_pricing
            .items
            .iter()
            .map(|m| (m, m.items_by_world()))
            .fold(HashMap::new(), |mut map, (item, item_map)| {
                for (world, listings) in item_map {
                    map.entry(world).or_default().push((item, listings));
                }
                map
            });

    /*for item in &best_pricing.items {
        let group_by = item.items_by_world();

        let status = match item.listing_status {
            ListingStatus::Good => '✅',
            ListingStatus::PartialFill => '⚠',
            ListingStatus::Unavailable => '❌',
        };
        println!("Item: {} {} \nitems:", item.name, status);
        item.market_ingredients.iter().for_each(|m| println!("seller: {} quantity: {} total: {} price per unit: {}", m.retainer_name, m.quantity.unwrap_or_default(), m.total, m.price_per_unit.unwrap_or_default()));

    }*/

    for (world, items) in &world_to_item_map {
        println!("{:<20}", world);
        for (pricing, listings) in items {
            let total: u32 = listings.iter().map(|m| m.total).sum();
            println!(
                "     ⚒ {:<20} \n   Retainer       QuantityxMGP\n{}",
                pricing.name,
                listings
                    .iter()
                    .map(|m| format!(
                        "{:>20}: {:>2}x{}mgp",
                        m.retainer_name,
                        m.quantity.unwrap_or_default(),
                        m.price_per_unit
                            .unwrap_or_default()
                            .to_formatted_string(&Locale::en)
                    ))
                    .join("\n")
            );
            println!("               Total: {}mgp", total.to_formatted_string(&Locale::en));
        }
    }
    println!(
        "Total cost {} mgp. {} stops",
        best_pricing.total.to_formatted_string(&Locale::en),
        world_to_item_map.len()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
