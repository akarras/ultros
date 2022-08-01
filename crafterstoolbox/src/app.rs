use egui::{Color32, Grid, ScrollArea};
use recipepricecheck::{
    BestPricingForItem, BestPricingSummary, ItemListingsSummary, PricingArguments,
    RecipePricingRawData,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tokio::sync::mpsc::{Receiver, Sender};
use universalis::ListingView;
use xivapi::models::recipe::Recipe;
use xivapi::IndexRecord;

#[derive(Deserialize, Serialize, Debug)]
struct RecipePriceList {
    recipe: Recipe,
    pricing_args: PricingArguments,
    data: Option<DataCollection>,
}

impl RecipePriceList {
    /// Used for immediate changes to PricingArguments
    fn update_data(&mut self) {
        if let Some(d) = &mut self.data {
            d.pricing_for_item = d
                .raw_data
                .create_best_price_summary(&self.pricing_args)
                .unwrap();
            d.world_to_item_map = d.pricing_for_item.get_items_by_world_cloned();
        }
    }

    /// Requests more data from Universalis
    fn refresh(
        &self,
        channel: &mut Option<(Sender<AppTx>, Receiver<AppRx>)>,
        data_center: impl ToString,
    ) {
        let recipe = self.recipe.clone();
        if let Some((sender, _)) = channel {
            sender
                .blocking_send(AppTx::RequestRecipe {
                    recipe,
                    data_center: data_center.to_string(),
                })
                .unwrap();
        }
    }
}

#[derive(Deserialize, Serialize, Debug)]
struct DataCollection {
    raw_data: RecipePricingRawData,
    pricing_for_item: BestPricingSummary,
    world_to_item_map: BTreeMap<String, Vec<(BestPricingForItem, Vec<ListingView>)>>,
    datacenter_pricing: ItemListingsSummary,
    home_world_pricing: Option<ItemListingsSummary>,
}

#[derive(Deserialize, Serialize, Default)]
struct CraftsList {
    windows: Vec<RecipePriceList>,
}

#[derive(Debug)]
pub enum AppTx {
    RequestRecipe { recipe: Recipe, data_center: String },
}

#[derive(Debug)]
pub enum AppRx {
    RecipeResponse {
        recipe_id: i64,
        raw_data: RecipePricingRawData,
    },
}

#[derive(Deserialize, Serialize, Default)]
#[serde(default)]
struct UserData {
    home_world: String,
}

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[derive(Deserialize, Serialize, Default)]
#[serde(default)] // if we add new fields, give them default values when deserializing old state
pub struct CraftersToolbox {
    crafts: CraftsList,
    #[serde(skip)]
    network_channel: Option<(Sender<AppTx>, Receiver<AppRx>)>,
    // Example stuff:
    recipe_query: String,
    recipe_query_results: Vec<usize>,
    #[serde(skip)]
    recipes: Vec<IndexRecord>,
    user_data: UserData,
    data_center: String,
}

impl CraftersToolbox {
    /// Called once before the first frame.
    pub fn new(
        recipes: Vec<IndexRecord>,
        network_channel: (Sender<AppTx>, Receiver<AppRx>),
        cc: &eframe::CreationContext<'_>,
    ) -> Self {
        // This is also where you can customized the look at feel of egui using
        // `cc.egui_ctx.set_visuals` and `cc.egui_ctx.set_fonts`.

        // Load previous app state (if any).
        // Note that you must enable the `persistence` feature for this to work.
        if let Some(storage) = cc.storage {
            let mut value: CraftersToolbox =
                eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();

            value.crafts.windows = value
                .crafts
                .windows
                .into_iter()
                .filter(|e| e.data.is_some())
                .collect();
            value.recipes = recipes;
            value.network_channel = Some(network_channel);
            return value;
        }

        Self {
            recipes,
            network_channel: Some(network_channel),
            ..Default::default()
        }
    }
}

impl eframe::App for CraftersToolbox {
    /// Called each time the UI needs repainting, which may be many times per second.
    /// Put your widgets into a `SidePanel`, `TopPanel`, `CentralPanel`, `Window` or `Area`.
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let Self {
            network_channel,
            recipe_query,
            recipe_query_results,
            recipes,
            crafts,
            user_data,
            data_center: datacenter,
        } = self;

        if let Some((_, rx)) = network_channel {
            if let Ok(value) = rx.try_recv() {
                match value {
                    AppRx::RecipeResponse {
                        recipe_id,
                        raw_data,
                    } => {
                        if let Some(value) = crafts
                            .windows
                            .iter_mut()
                            .find(|window| window.recipe.id == recipe_id)
                        {
                            let best_pricing = raw_data
                                .create_best_price_summary(&value.pricing_args)
                                .unwrap();
                            let world_data = best_pricing.get_items_by_world_cloned();
                            let datacenter_pricing =
                                raw_data.get_recipe_target_item_listing_summary().unwrap();
                            let home_world_pricing = if !user_data.home_world.is_empty() {
                                raw_data
                                    .get_recipe_target_pricing_for_world(&user_data.home_world)
                                    .ok()
                            } else {
                                None
                            };
                            value.data = Some(DataCollection {
                                raw_data,
                                pricing_for_item: best_pricing,
                                world_to_item_map: world_data,
                                datacenter_pricing,
                                home_world_pricing,
                            });
                        } else {
                            panic!("Failed to find window");
                        }
                    }
                }
            }
        }

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            // The top panel is often a good place for a menu bar:
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Quit").clicked() {
                        frame.quit();
                    }
                });
                ui.menu_button("Crafter", |ui| if ui.button("Attributes").clicked() {})
            });
        });

        egui::SidePanel::left("side_panel")
            .resizable(false)
            .show(ctx, |ui| {
                ui.heading("Recipe Lookup");
                ui.label("datacenter:");
                ui.text_edit_singleline(datacenter);
                ui.label("home world:");
                ui.text_edit_singleline(&mut user_data.home_world);
                ui.label("recipe search: ");
                if ui.text_edit_singleline(recipe_query).changed() {
                    let lower = recipe_query.to_lowercase();
                    *recipe_query_results = recipes
                        .iter()
                        .enumerate()
                        .filter(|(_, m)| m.name.to_lowercase().find(&lower).is_some())
                        .map(|(i, _)| i)
                        .collect()
                }
                ScrollArea::vertical().show(ui, |ui| {
                    let recipes: Box<dyn Iterator<Item = &IndexRecord>> =
                        if recipe_query_results.is_empty() {
                            Box::new(recipes.iter())
                        } else {
                            Box::new(recipe_query_results.iter().map(|i| &recipes[*i as usize]))
                        };
                    for recipe in recipes.take(20) {
                        ui.horizontal(|ui| {
                            ui.label(&recipe.name);
                            if ui.button("Craft").clicked() {
                                crafts.add_recipe(
                                    recipe.id,
                                    network_channel,
                                    datacenter.to_string(),
                                );
                            }
                        });
                    }
                });

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 7.5;
                        ui.label("crafters toolbox by chew 💖 powered by");
                        ui.hyperlink_to("universalis", "https://universalis.app");
                        ui.hyperlink_to("garland tools", "https://garlandtools.org/");
                        ui.hyperlink_to("xivapi", "https://xivapi.com")
                    });
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            // The central panel the region left after adding TopPanel's and SidePanel's
            egui::warn_if_debug_build(ui);
        });

        let mut remove_id = None;
        for (i, buddy) in crafts.windows.iter_mut().enumerate() {
            egui::Window::new(buddy.recipe.name.as_ref().unwrap())
                .default_width(400.0)
                .resizable(true)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        ui.label("Quantity: ");
                        if ui
                            .add(
                                egui::DragValue::new(&mut buddy.pricing_args.craft_quantity)
                                    .clamp_range(1..=1000)
                                    .speed(1.0),
                            )
                            .changed()
                        {
                            // update data
                            buddy.update_data();
                        }
                        if ui
                            .checkbox(&mut buddy.pricing_args.filter_shards, "Filter Shards")
                            .changed()
                        {
                            buddy.update_data();
                        }
                        if ui
                            .checkbox(
                                &mut buddy.pricing_args.filter_items_with_too_much_quantity,
                                "Filter large stacks",
                            )
                            .changed()
                        {
                            buddy.update_data();
                        }
                        if ui.button("🔃").clicked() {
                            buddy.refresh(network_channel, datacenter.to_string());
                        }
                        if ui.button("❌").clicked() {
                            remove_id = Some(i)
                        }
                    });
                    if let Some(data) = &buddy.data {
                        let world_map = &data.world_to_item_map;
                        ScrollArea::vertical().max_height(600.0).show(ui, |ui| {
                            Grid::new(buddy.recipe.id)
                                .num_columns(3)
                                .spacing([40.0, 5.0])
                                .striped(true)
                                .show(ui, |ui| {
                                    for (world, items) in world_map {
                                        let world_color = Color32::from_rgb(0, 150, 10);
                                        ui.colored_label(world_color, "World: ");
                                        ui.colored_label(world_color, world);
                                        ui.end_row();
                                        for (item, listings) in items {
                                            let item_color = Color32::from_rgb(0, 0, 255);
                                            ui.colored_label(item_color, "item");
                                            ui.colored_label(item_color, &item.name);
                                            ui.colored_label(item_color, "HQ");
                                            ui.colored_label(item_color, "quantity");
                                            ui.colored_label(item_color, "price per item");
                                            ui.end_row();
                                            for listing in listings {
                                                ui.label("retainer");
                                                ui.label(&listing.retainer_name);
                                                ui.label(match listing.hq {
                                                    true => "✅",
                                                    false => "",
                                                });
                                                ui.label(listing.quantity.unwrap_or(0).to_string());
                                                ui.label(
                                                    listing
                                                        .price_per_unit
                                                        .unwrap_or(9999999)
                                                        .to_string(),
                                                );
                                                ui.end_row();
                                            }
                                        }
                                    }
                                });
                        });
                        let pricing = &data.pricing_for_item;
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 5.0;
                            ui.label("gil total: ");
                            ui.label(pricing.total.to_string());
                            ui.label("items total: ");
                            ui.label(pricing.items.len().to_string())
                        });
                        let pricing_summary = &data.datacenter_pricing;
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 5.0;
                            ui.label("datacenter ");
                            if let Some(lq) = &pricing_summary.lq_items {
                                ui.label("LQ: ");
                                ui.label(lq.to_string());
                            }
                            if let Some(hq) = &pricing_summary.hq_items {
                                ui.label("HQ: ");
                                ui.label(hq.to_string());
                            }
                        });
                        if let Some(pricing_summary) = &data.home_world_pricing {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 5.0;
                                ui.label(user_data.home_world.as_str());
                                if let Some(lq) = &pricing_summary.lq_items {
                                    ui.label("LQ: ");
                                    ui.label(lq.to_string());
                                }
                                if let Some(hq) = &pricing_summary.hq_items {
                                    ui.label("HQ: ");
                                    ui.label(hq.to_string());
                                }
                            });
                        }
                    } else {
                        ui.spinner();
                    }
                });
        }
        if let Some(remove_id) = remove_id {
            crafts.windows.remove(remove_id);
        }
    }

    /// Called by the frame work to save state before shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }
}

impl CraftsList {
    fn add_recipe(
        &mut self,
        recipe_id: i64,
        network_channel: &mut Option<(Sender<AppTx>, Receiver<AppRx>)>,
        data_center: impl ToString,
    ) {
        let recipe = xivapi::disk_query(xivapi::RecipeRequest::new(recipe_id as u32)).unwrap();
        if let Some((tx, _)) = network_channel {
            tx.blocking_send(AppTx::RequestRecipe {
                recipe: recipe.clone(),
                data_center: data_center.to_string(),
            })
            .unwrap();
        }
        let pricing_buddy = RecipePriceList {
            recipe,
            data: None,
            pricing_args: PricingArguments::default(),
        };
        self.windows.push(pricing_buddy);
    }
}
