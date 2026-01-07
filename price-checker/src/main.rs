use clap::Parser;
use universalis::{MarketView, UniversalisClient};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// World or Datacenter name
    #[arg(short, long)]
    world: String,

    /// Item ID to check
    #[arg(short, long)]
    item_id: i32,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let client = UniversalisClient::new("price-checker");

    println!(
        "Checking price for item {} on {}...",
        args.item_id, args.world
    );

    let market_data = client
        .marketboard_current_data(&args.world, &[args.item_id])
        .await?;

    match market_data {
        MarketView::SingleView(view) => {
            if let Some(cheapest) = view.listings.first() {
                println!("Cheapest listing:");
                println!("  Price: {:?} gil", cheapest.price_per_unit.unwrap_or(0));
                println!(
                    "  World: {:?}",
                    cheapest.world_name.as_deref().unwrap_or("Unknown")
                );
                println!("  Quantity: {:?}", cheapest.quantity.unwrap_or(0));
                println!("  Total: {:?} gil", cheapest.total);
            } else {
                println!("No listings found.");
            }
        }
        MarketView::MultiView(_) => {
            println!("Received multiview, expected single view.");
        }
    }

    Ok(())
}
