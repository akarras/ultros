use crate::UniversalisData;
use bincode::config::Configuration;
use egui::{Color32, Grid, ScrollArea, Visuals, Widget};
use flate2::FlushDecompress;
use futures::StreamExt;
use lazy_static::lazy_static;
use recipepricecheck::{
    BestPricingForItem, BestPricingSummary, ItemListingsSummary, PricingArguments,
    RecipePricingRawData,
};
use serde::{Deserialize, Serialize, Serializer};
use serde_error::Error;
use std::collections::{BTreeMap, HashMap};
use std::fmt::{Display, Formatter};
use std::task::Poll;
use tokio::sync::mpsc::{Receiver, Sender};
use universalis::{
    CurrentlyShownSingleView, DataCenterName, ListingView, MarketView, RegionName, WorldName,
};
use xiv_gen::Recipe;
use xiv_gen::RecipeId;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Deserialize, Serialize, Debug)]
struct RecipePriceList {
    recipe_id: RecipeId,
    pricing_args: PricingArguments,
    data: Option<DataCollection>,
}

impl RecipePriceList {
    /// Used for immediate changes to PricingArguments
    fn update_data(&mut self, data: &xiv_gen::Data) {
        if let Some(d) = &mut self.data {
            if let Ok(raw_data) = &d.raw_data {
                d.pricing_for_item = raw_data
                    .create_best_price_summary(&self.pricing_args, data)
                    .map_err(|e| Error::new(&*anyhow::Error::from(e)));
                d.world_by_item_pricing = d
                    .pricing_for_item
                    .as_ref()
                    .map(|m| m.get_items_by_world_cloned())
                    .unwrap_or_default();
            }
        }
    }

    /// Requests more data from Universalis
    fn refresh(
        &self,
        channel: &mut Option<(Sender<AppTx>, Receiver<AppRx>)>,
        data_center: impl ToString,
    ) {
        if let Some((sender, _)) = channel {
            sender
                .blocking_send(AppTx::RequestRecipe {
                    recipe_id: self.recipe_id,
                    data_center: data_center.to_string(),
                })
                .expect("tokio sender error, unrecoverable aborting");
        }
    }
}

#[derive(Deserialize, Serialize, Debug)]
struct DataCollection {
    raw_data: Result<RecipePricingRawData>,
    pricing_for_item: Result<BestPricingSummary>,
    world_by_item_pricing: BTreeMap<String, Vec<(BestPricingForItem, Vec<ListingView>)>>,
    datacenter_pricing: Option<ItemListingsSummary>,
    home_world_pricing: Option<ItemListingsSummary>,
}

#[derive(Deserialize, Serialize, Default, Debug)]
struct CraftsList {
    windows: Vec<RecipePriceList>,
}

#[derive(Debug)]
pub enum AppTx {
    RequestRecipe {
        recipe_id: RecipeId,
        data_center: String,
    },
}

#[derive(Debug)]
pub enum AppRx {
    RecipeResponse {
        recipe_id: RecipeId,
        raw_data: core::result::Result<RecipePricingRawData, universalis::Error>,
    },
    UniversalisData {
        universalis_data: UniversalisData,
    },
}

#[derive(Deserialize, Serialize, Default, Debug)]
#[serde(default)]
struct UserData {
    region: Option<RegionName>,
    data_center: Option<DataCenterName>,
    home_world: Option<WorldName>,
    crafters: Crafters,
}

#[derive(Deserialize, Serialize, Default, Debug)]
#[serde(default)]
struct CrafterDetails {
    cp: u32,
    control: u32,
    craftsmanship: u32,
    level: u32,
}

#[derive(Deserialize, Serialize, Default, Debug)]
#[serde(default)]
struct Crafters {
    carpenter: CrafterDetails,
    blacksmith: CrafterDetails,
    armorer: CrafterDetails,
    goldsmith: CrafterDetails,
    leatherworker: CrafterDetails,
    weaver: CrafterDetails,
    alchemist: CrafterDetails,
    culinarian: CrafterDetails,
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy)]
pub enum CraftJob {
    Carpenter,
    Blacksmith,
    Armorer,
    Goldsmith,
    Leatherworker,
    Weaver,
    Alchemist,
    Culinarian,
}

impl Display for CraftJob {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                CraftJob::Carpenter => "CRP",
                CraftJob::Blacksmith => "BSM",
                CraftJob::Armorer => "ARM",
                CraftJob::Goldsmith => "GSM",
                CraftJob::Leatherworker => "LTW",
                CraftJob::Weaver => "WVR",
                CraftJob::Alchemist => "ALC",
                CraftJob::Culinarian => "CUL",
            }
        )
    }
}

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[derive(Deserialize, Serialize, Debug)]
#[serde(default)] // if we add new fields, give them default values when deserializing old state
pub struct CraftersToolbox {
    crafts: CraftsList,
    #[serde(skip)]
    network_channel: Option<(Sender<AppTx>, Receiver<AppRx>)>,
    // Example stuff:
    recipe_query: String,
    user_data: UserData,
    query_target: String,
    /// Holds data about datacenters and worlds
    #[serde(skip)]
    universalis_data: UniversalisData,
    /// Holds the query results for the previous recipe query
    #[serde(skip)]
    recipe_query_results: Vec<(RecipeId, String, Vec<CraftJob>)>,
    #[serde(skip)]
    game_data: &'static xiv_gen::Data,
    #[serde(skip)]
    recipes: Vec<(RecipeId, String, Vec<CraftJob>)>,
}

impl Default for CraftersToolbox {
    fn default() -> Self {
        Self {
            crafts: Default::default(),
            network_channel: None,
            recipe_query: "".to_string(),
            recipe_query_results: vec![],
            user_data: Default::default(),
            query_target: "".to_string(),
            game_data: CraftersToolbox::decompress_data(),
            recipes: vec![],
            universalis_data: UniversalisData::default(),
        }
    }
}

impl CraftersToolbox {
    /// Called once before the first frame.
    pub fn new(
        mut network_channel: (Sender<AppTx>, Receiver<AppRx>),
        cc: &eframe::CreationContext<'_>,
    ) -> Self {
        // This is also where you can customized the look at feel of egui using
        // `cc.egui_ctx.set_visuals` and `cc.egui_ctx.set_fonts`.
        cc.egui_ctx.set_visuals(Visuals::dark());
        // Load previous app state (if any).
        // Note that you must enable the `persistence` feature for this to work.
        let recipes = Self::create_recipe_list();
        let universalis_data = match network_channel.1.blocking_recv().unwrap() {
            AppRx::UniversalisData { universalis_data } => universalis_data,
            _ => panic!("Expected universalis data"),
        };

        if let Some(storage) = cc.storage {
            let mut value: CraftersToolbox =
                eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();
            value.crafts.windows = value
                .crafts
                .windows
                .into_iter()
                .filter(|e| e.data.is_some())
                .map(|mut m| {
                    m.update_data(&value.game_data);
                    m
                })
                .collect();
            value.network_channel = Some(network_channel);
            value.recipes = recipes;
            Self::update_search(
                &value.recipe_query,
                &value.recipes,
                &mut value.recipe_query_results,
            );
            value.universalis_data = universalis_data;
            return value;
        }

        Self {
            network_channel: Some(network_channel),
            recipes,
            universalis_data,
            ..Default::default()
        }
    }

    fn try_insert_recipe(
        map: &mut HashMap<RecipeId, Vec<CraftJob>>,
        recipe_id: RecipeId,
        crafter: CraftJob,
    ) {
        if recipe_id.inner() == 0 {
            return;
        }
        map.entry(recipe_id).or_default().push(crafter);
    }

    fn create_recipe_list() -> Vec<(RecipeId, String, Vec<CraftJob>)> {
        // this might be good to store somewhere
        let game_data = Self::decompress_data();
        let recipes = game_data.get_recipes();
        let items = game_data.get_items();
        let recipe_lookup = game_data.get_recipe_lookups();
        let mut jobs: HashMap<RecipeId, Vec<CraftJob>> =
            recipe_lookup
                .values()
                .fold(HashMap::new(), |mut map, lookup| {
                    Self::try_insert_recipe(&mut map, lookup.get_crp(), CraftJob::Carpenter);
                    Self::try_insert_recipe(&mut map, lookup.get_bsm(), CraftJob::Blacksmith);
                    Self::try_insert_recipe(&mut map, lookup.get_arm(), CraftJob::Armorer);
                    Self::try_insert_recipe(&mut map, lookup.get_gsm(), CraftJob::Goldsmith);
                    Self::try_insert_recipe(&mut map, lookup.get_ltw(), CraftJob::Leatherworker);
                    Self::try_insert_recipe(&mut map, lookup.get_wvr(), CraftJob::Weaver);
                    Self::try_insert_recipe(&mut map, lookup.get_alc(), CraftJob::Alchemist);
                    Self::try_insert_recipe(&mut map, lookup.get_cul(), CraftJob::Culinarian);
                    map
                });
        recipes
            .values()
            .map(|r| (r.get_key_id(), r.get_item_result()))
            .filter(|(_id, result)| result.inner() != 0)
            .map(|(recipe_id, item_id)| {
                (
                    recipe_id,
                    items
                        .get(&item_id)
                        .expect(&format!("unable to get item_id: {}", item_id.inner())),
                )
            })
            .map(|(recipe_id, item)| {
                (
                    recipe_id,
                    item.get_name(),
                    jobs.remove(&recipe_id).unwrap_or_default(),
                )
            })
            .collect()
    }

    pub fn decompress_data() -> &'static xiv_gen::Data {
        fn decompress_impl() -> xiv_gen::Data {
            let mut decompressor = flate2::Decompress::new(true);
            let mut data = Vec::new();
            let bytes = include_bytes!("../../database.bincode");
            data.reserve(bytes.len() * 5);
            decompressor
                .decompress_vec(bytes, &mut data, FlushDecompress::Sync)
                .unwrap();
            let (data, _) =
                bincode::decode_from_slice(data.as_slice(), bincode::config::standard()).unwrap();
            data
        }
        lazy_static! {
            pub static ref XIV_DATA: xiv_gen::Data = decompress_impl();
        }
        &XIV_DATA
    }

    fn update_search(
        recipe_query: &String,
        recipes: &Vec<(xiv_gen::RecipeId, String, Vec<CraftJob>)>,
        recipe_query_results: &mut Vec<(xiv_gen::RecipeId, String, Vec<CraftJob>)>,
    ) {
        let lower = recipe_query.to_lowercase();

        *recipe_query_results = recipes
            .iter()
            .filter(|(_, name, _)| name.to_lowercase().find(&lower).is_some())
            .cloned()
            .collect()
    }
}

fn add_disabled_button(ui: &mut egui::Ui, target: &mut String, src: &str) {
    ui.scope(|ui| {
        ui.set_enabled(*target != src);
        if ui.button(src).clicked() {
            *target = src.to_string();
        }
    });
}

fn create_crafter_menu(ui: &mut egui::Ui, crafter_details: &mut CrafterDetails) {
    let values = [
        ("craftsmanship: ", &mut crafter_details.craftsmanship),
        ("control: ", &mut crafter_details.control),
        ("cp: ", &mut crafter_details.cp),
        ("level: ", &mut crafter_details.level),
    ];
    for (label, value) in values {
        ui.label(label);
        egui::DragValue::new(value).ui(ui);
    }
}

fn draw_err<'a, T>(data: &'a Result<T>, ui: &'_ mut egui::Ui) -> Option<&'a T> {
    if let Err(e) = data {
        ui.label(format!("{}", e));
    }
    data.as_ref().ok()
}

impl<'a> eframe::App for CraftersToolbox {
    /// Called each time the UI needs repainting, which may be many times per second.
    /// Put your widgets into a `SidePanel`, `TopPanel`, `CentralPanel`, `Window` or `Area`.
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let Self {
            network_channel,
            recipe_query,
            universalis_data,
            recipe_query_results,
            crafts,
            user_data,
            query_target,
            game_data,
            recipes,
        } = self;
        let UserData {
            region,
            data_center,
            home_world,
            crafters,
        } = user_data;
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
                            .find(|window| window.recipe_id == recipe_id)
                        {
                            let raw_data = raw_data.map_err(|e| Error::new(&e));
                            // todo this sucks
                            let pricing_for_item = if let Some(pricing_for_item) = raw_data.as_ref().ok().map(|raw_data| {
                                raw_data
                                    .create_best_price_summary(&value.pricing_args, game_data)
                                    .map_err(|e| e.into())
                            }) {
                                pricing_for_item
                            } else {
                                Err(anyhow::Error::msg("No raw data to work from"))
                            }.map_err(|e| serde_error::Error::new(&*e));
                            let world_by_item_pricing = pricing_for_item
                                .as_ref()
                                .map(|m| m.get_items_by_world_cloned())
                                .unwrap_or_default();
                            let datacenter_pricing = raw_data
                                .as_ref()
                                .map_err(|e| Error::new(&*e))
                                .and_then(|m| m.get_recipe_target_item_listing_summary().map_err(|e| Error::new(&e)));
                            let home_world_pricing = if let Some(home_world) = home_world {
                                raw_data
                                    .as_ref()
                                    .ok()
                                    .map(|m| {
                                        m.get_recipe_target_pricing_for_world(&home_world.0).ok()
                                    })
                                    .flatten()
                            } else {
                                None
                            };
                            value.data = Some(DataCollection {
                                raw_data,
                                pricing_for_item,
                                world_by_item_pricing,
                                datacenter_pricing: datacenter_pricing.ok(),
                                home_world_pricing,
                            });
                        } else {
                            panic!("Failed to find window");
                        }
                    }
                    AppRx::UniversalisData {
                        universalis_data: data,
                    } => {
                        *universalis_data = data;
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

                ui.menu_button("Crafter", |ui| {
                    let Crafters {
                        carpenter,
                        blacksmith,
                        armorer,
                        goldsmith,
                        leatherworker,
                        weaver,
                        alchemist,
                        culinarian,
                    } = crafters;
                    for (crafter_name, craft_details) in [
                        ("carpenter", carpenter),
                        ("blacksmith", blacksmith),
                        ("armorer", armorer),
                        ("goldsmith", goldsmith),
                        ("leatherworker", leatherworker),
                        ("weaver", weaver),
                        ("alchemist", alchemist),
                        ("culinarian", culinarian),
                    ] {
                        ui.menu_button(crafter_name, |ui| {
                            create_crafter_menu(ui, craft_details);
                        });
                    }
                });
                ui.menu_button("Home world settings", |ui| {
                    egui::ComboBox::from_label("Region")
                        .selected_text(format!(
                            "{}",
                            region
                                .as_mut()
                                .unwrap_or(&mut RegionName("No Region".to_string()))
                                .0
                        ))
                        .show_ui(ui, |ui| {
                            for (r, _) in &universalis_data.regions {
                                ui.selectable_value(region, Some(r.clone()), &r.0);
                            }
                        });
                    if let Some(dcs) = region
                        .as_ref()
                        .and_then(|selected_region| universalis_data.regions.get(selected_region))
                    {
                        egui::ComboBox::from_label("Datacenter")
                            .selected_text(format!(
                                "{}",
                                data_center
                                    .as_mut()
                                    .unwrap_or(&mut DataCenterName("No Datacenter".to_string()))
                                    .0
                            ))
                            .show_ui(ui, |ui| {
                                for dc in dcs {
                                    ui.selectable_value(data_center, Some(dc.clone()), &dc.0);
                                }
                            });
                    }
                    if let Some(worlds) = data_center
                        .as_ref()
                        .and_then(|selected_dc| universalis_data.data_centers.get(selected_dc))
                    {
                        egui::ComboBox::from_label("Home World")
                            .selected_text(format!(
                                "{}",
                                home_world
                                    .as_mut()
                                    .unwrap_or(&mut WorldName("No Homeworld".to_string()))
                                    .0
                            ))
                            .show_ui(ui, |ui| {
                                for w in worlds {
                                    ui.selectable_value(home_world, Some(w.clone()), &w.0);
                                }
                            });
                    }
                });

                    if let Some(region) = region {
                ui.menu_button(&format!("marketboard filter: {query_target}"), |ui| {
                        add_disabled_button(ui, query_target, &region.0);
                    if let Some(data_center) = data_center {
                        add_disabled_button(ui, query_target, &data_center.0);
                        if let Some(worlds) = universalis_data.data_centers.get(data_center) {
                            for world in worlds {
                                add_disabled_button(ui, query_target, &world.0);
                            }
                        }
                    }
                });
                    }
            });
        });

        egui::SidePanel::left("side_panel")
            .default_width(250.0)
            .resizable(false)
            .show(ctx, |ui| {
                ui.heading("Recipe Lookup");
                if ui.text_edit_singleline(recipe_query).changed() {
                    Self::update_search(recipe_query, recipes, recipe_query_results)
                }
                ScrollArea::vertical().show_rows(ui, 15.0, recipe_query_results.len(), |ui, range| {
                    for i in range {
                        let (id, item_name, jobs) = &recipe_query_results[i];
                        ui.horizontal(|ui| {
                            ui.label(item_name.as_str());
                            ui.with_layout(egui::Layout::right_to_left(), |ui| {
                                ui.scope(|ui| {
                                    let already_open =
                                        crafts.windows.iter().any(|list| *id == list.recipe_id);
                                    ui.set_enabled(!already_open);
                                    if ui.button("💲").clicked() {
                                        crafts.add_recipe(
                                            *id,
                                            network_channel,
                                            game_data,
                                            query_target.to_string(),
                                        );
                                    }
                                });
                                if ui.button("⚒").clicked() {
                                    println!("todo implement");
                                }
                                for job in jobs {
                                    ui.label(&format!("[{job}]"));
                                }
                            });
                        });
                    }
                });

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.spacing_mut().item_spacing.x = 7.5;
                    ui.horizontal(|ui| {
                        ui.hyperlink_to("universalis", "https://universalis.app");
                        ui.hyperlink_to("garland tools", "https://garlandtools.org/");
                        ui.hyperlink_to("xivapi", "https://xivapi.com")
                    });
                    ui.label("crafters toolbox by chew 💖 powered by");
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            // The central panel the region left after adding TopPanel's and SidePanel's
            egui::warn_if_debug_build(ui);
        });

        let mut remove_id = None;
        for (i, buddy) in crafts.windows.iter_mut().enumerate() {
            let items = game_data.get_items();
            let recipes = game_data.get_recipes();
            let recipe = recipes.get(&buddy.recipe_id).unwrap();
            let item_id = recipe.get_item_result();
            let item = items.get(&item_id).unwrap();
            egui::Window::new(&format!("craft price: {}", item.get_name()))
                .default_width(400.0)
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
                            buddy.update_data(game_data);
                        }
                        if ui
                            .checkbox(&mut buddy.pricing_args.filter_shards, "Filter Shards")
                            .changed()
                        {
                            buddy.update_data(game_data);
                        }
                        if ui
                            .checkbox(
                                &mut buddy.pricing_args.filter_items_with_too_much_quantity,
                                "Filter large stacks",
                            )
                            .changed()
                        {
                            buddy.update_data(game_data);
                        }
                        if ui.button("🔃").clicked() {
                            buddy.refresh(network_channel, query_target.to_string());
                        }
                        if ui.button("❌").clicked() {
                            remove_id = Some(i)
                        }
                    });
                    if let Some(data) = &buddy.data {
                        let world_map = &data.world_by_item_pricing;
                        ScrollArea::vertical().max_height(600.0).show(ui, |ui| {
                            Grid::new(buddy.recipe_id)
                                .num_columns(3)
                                .spacing([40.0, 5.0])
                                .striped(true)
                                .show(ui, |ui| {
                                    for (world, items) in world_map {
                                        let world_color = Color32::from_rgb(69, 199, 19);
                                        ui.colored_label(world_color, "World: ");
                                        ui.colored_label(world_color, world);
                                        ui.end_row();
                                        for (item, listings) in items {
                                            let item_color = Color32::from_rgb(100, 50, 210);
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
                        if let Some(pricing) = draw_err(&data.pricing_for_item, ui) {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 5.0;
                                ui.label("craft ingredient gil total: ");
                                ui.label(pricing.total.to_string());
                                ui.label("items total: ");
                                ui.label(pricing.items.len().to_string())
                            });
                            if let Some(pricing_summary) = &data.datacenter_pricing {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 5.0;
                                    ui.label(query_target.as_str());
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
                            if let Some(pricing_summary) = &data.home_world_pricing {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 5.0;
                                    ui.label(
                                        user_data
                                            .home_world
                                            .as_ref()
                                            .unwrap_or(&WorldName("invalid".to_string()))
                                            .0
                                            .as_str(),
                                    );
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
        recipe_id: RecipeId,
        network_channel: &mut Option<(Sender<AppTx>, Receiver<AppRx>)>,
        data: &xiv_gen::Data,
        data_center: impl ToString,
    ) {
        // let recipe = data.get_recipes().get(&recipe_id).unwrap();
        if let Some((tx, _)) = network_channel {
            tx.blocking_send(AppTx::RequestRecipe {
                recipe_id: recipe_id,
                data_center: data_center.to_string(),
            })
            .unwrap();
        }
        let pricing_buddy = RecipePriceList {
            recipe_id: recipe_id,
            data: None,
            pricing_args: PricingArguments::default(),
        };
        self.windows.push(pricing_buddy);
    }
}
