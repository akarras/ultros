#![warn(clippy::all, rust_2018_idioms)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use crafterstoolbox::{AppRx, AppTx, CraftersToolbox, UniversalisData};
use log::info;
use universalis::UniversalisClient;

// When compiling natively:
#[cfg(not(target_arch = "wasm32"))]
fn main() {
    // Log to stdout (if you run with `RUST_LOG=debug`).
    tracing_subscriber::fmt::init();
    let native_options = eframe::NativeOptions::default();
    let (app_tx_sender, mut app_tx_receiver) = tokio::sync::mpsc::channel(10);
    let (app_rx_sender, app_rx_receiver) = tokio::sync::mpsc::channel(10);
    let data = xiv_gen_db::decompress_data();
    info!("Starting network thread");

    std::thread::scope(move |s| {
        s.spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            info!("starting tokio");
            runtime.block_on(async move {
                info!("runtime begin");
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
                                region_datacenter_or_server: datacenter,
                            } => {
                                let recipes = data.get_recipes();
                                let recipe = recipes.get(&recipe_id).unwrap();
                                let pricing = recipepricecheck::get_ingredient_prices(
                                    &client,
                                    &datacenter,
                                    recipe,
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
                            AppTx::RequestItem {
                                item_id,
                                region_datacenter_or_server,
                            } => {
                                let item_ids = [item_id.inner()];
                                let (market_view, history_view) = futures::future::join(
                                    client.marketboard_current_data(
                                        &region_datacenter_or_server,
                                        &item_ids,
                                    ),
                                    client
                                        .get_item_history(&region_datacenter_or_server, &item_ids),
                                )
                                .await;
                                app_rx_sender
                                    .send(AppRx::ItemResponse {
                                        item_id,
                                        market_view,
                                        history_view,
                                    })
                                    .await
                                    .unwrap();
                            }
                        }
                    } else {
                        break;
                    }
                }
            })
        });
        info!("initiating gui");
        eframe::run_native(
            "crafters toolbox",
            native_options,
            Box::new(|cc| Box::new(CraftersToolbox::new((app_tx_sender, app_rx_receiver), cc))),
        );
    });
}
