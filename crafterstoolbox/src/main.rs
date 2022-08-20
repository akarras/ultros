#![warn(clippy::all, rust_2018_idioms)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use crafterstoolbox::{AppRx, AppTx, CraftersToolbox, UniversalisData};
use recipepricecheck::PricingArguments;
use tokio::sync::mpsc::Sender;
use universalis::UniversalisClient;
use xiv_gen::RecipeId;

// When compiling natively:
#[cfg(not(target_arch = "wasm32"))]
fn main() {
    // Log to stdout (if you run with `RUST_LOG=debug`).
    tracing_subscriber::fmt::init();
    let native_options = eframe::NativeOptions::default();
    println!("init tx");
    let (app_tx_sender, mut app_tx_receiver) = tokio::sync::mpsc::channel(10);
    let (app_rx_sender, app_rx_receiver) = tokio::sync::mpsc::channel(10);
    let data = CraftersToolbox::decompress_data();
    println!("spawn thread");

    std::thread::scope(move |s| {
        s.spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            println!("starting tokio");
            runtime.block_on(async move {
                println!("runtime begin");
                let universalis_data = UniversalisData::initialize_data().await;
                app_rx_sender
                    .send(AppRx::UniversalisData { universalis_data })
                    .await
                    .unwrap();
                let client = UniversalisClient::new();
                loop {
                    if let Some(value) = app_tx_receiver.recv().await {
                        match value {
                            AppTx::RequestRecipe {
                                recipe_id,
                                data_center: datacenter,
                            } => {
                                let recipes = data.get_recipes();
                                let recipe = recipes.get(&recipe_id).unwrap();
                                let pricing = recipepricecheck::get_ingredient_prices(
                                    &client,
                                    &datacenter,
                                    &recipe,
                                )
                                .await;
                                app_rx_sender
                                    .send(AppRx::RecipeResponse {
                                        recipe_id,
                                        raw_data: pricing,
                                    })
                                    .await
                                    .expect("cross thread IO error");
                            }
                        }
                    }
                }
            })
        });
        println!("crafters toolbox run");
        eframe::run_native(
            "crafters toolbox",
            native_options,
            Box::new(|cc| Box::new(CraftersToolbox::new((app_tx_sender, app_rx_receiver), cc))),
        );
    });
}
