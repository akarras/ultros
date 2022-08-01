use clap::Parser;
use console::Term;
use dialoguer::theme::ColorfulTheme;
use dialoguer::FuzzySelect;
use itertools::Itertools;
use num_format::{Locale, ToFormattedString};
use recipepricecheck::{
    get_ingredient_prices, BestPricingForItem, ListingStatus, PricingArguments,
    RecipePricingRawData,
};
use std::collections::HashMap;
use universalis::{ListingView, UniversalisClient};
use xivapi::{disk_query_async, RecipeRequest, XivDataType};

#[derive(Debug, Parser)]
#[clap(author, version, about)]
struct Args {
    #[clap(short, long, value_parser)]
    recipe_name: String,
    #[clap(short, long, value_parser)]
    world_name: String,
    #[clap(short, long, value_parser)]
    quantity: i64,
    #[clap(short, long, value_parser, default_value = "true")]
    filter_shards: bool,
    #[clap(long, value_parser, default_value = "false")]
    filter_items_with_too_much_quantity: bool,
    #[clap(short)]
    user_home_world: Option<String>,
}

#[tokio::main]
async fn main() {
    let args: Args = Args::parse();
    // find the recipe first
    let index = xivapi::get_index(&XivDataType::Recipe);

    /*let recipes : Vec<_> = index
    .search(&args.recipe_name)
    .collect();*/

    let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
        .items(&index.0)
        .default(0)
        .interact_on_opt(&Term::stderr())
        .unwrap()
        .unwrap();

    let search = index.0.get(selection).unwrap();

    let recipe = disk_query_async(RecipeRequest::new(search.id as u32))
        .await
        .unwrap();
    let client = UniversalisClient::new();
    let best_pricing = get_ingredient_prices(
        client,
        &args.world_name,
        recipe,
        args.quantity,
        &PricingArguments {
            filter_shards: args.filter_shards,
            filter_items_with_too_much_quantity: args.filter_items_with_too_much_quantity,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let world_to_item_map: HashMap<&String, Vec<(&BestPricingForItem, Vec<&ListingView>)>> =
        get_items_by_world(&best_pricing);

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
            println!(
                "               Total: {}mgp",
                total.to_formatted_string(&Locale::en)
            );
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
