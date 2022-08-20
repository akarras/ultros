use crate::crafting_types::{create_crafter_menu, CraftJob, Crafters};
use crate::sidepanel::item_panel::ItemPanel;
use crate::sidepanel::SidePanel;
use crate::UniversalisData;
use bincode::config::Configuration;
use egui::{Color32, Grid, ScrollArea, Visuals, Widget};
use fixed_decimal::FixedDecimal;
use flate2::FlushDecompress;
use futures::StreamExt;
use icu::decimal::options::{FixedDecimalFormatterOptions, GroupingStrategy};
use icu::decimal::FixedDecimalFormatter;
use icu::locid::locale;
use lazy_static::lazy_static;
use log::{info, warn};
use recipepricecheck::{
    BestPricingForItem, BestPricingSummary, CrafterDetails, ItemListingsSummary, PricingArguments,
    RecipePricingRawData,
};
use serde::{Deserialize, Serialize, Serializer};
use serde_error::Error;
use std::borrow::Borrow;
use std::collections::{BTreeMap, HashMap};
use std::fmt::{Display, Formatter};
use std::task::Poll;
use tokio::sync::mpsc::{Receiver, Sender};
use universalis::{
    CurrentlyShownMultiView, CurrentlyShownSingleView, DataCenterName, ListingView, MarketView,
    RegionName, WorldName,
};
use writeable::Writeable;
use xiv_gen::ItemId;
use xiv_gen::Recipe;
use xiv_gen::RecipeId;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Deserialize, Serialize, Debug)]
pub(crate) struct RecipePriceList {
    pub(crate) recipe_id: RecipeId,
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
                    region_datacenter_or_server: data_center.to_string(),
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

#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct ItemData {
    item_id: ItemId,
    hq_only: bool,
    state: ItemWindowDataState,
}

impl ItemData {
    pub(crate) fn update_query(&mut self) {
        if let ItemWindowDataState::Loaded { item_data, query_view, .. } = &mut self.state {
            *query_view = item_data.get_listings_for_item_id(self.item_id.inner() as u32).expect("Should always have this?").iter().filter(|item| !self.hq_only || item.hq).cloned().collect();
        }
    }

    pub(crate) fn refresh(&self, network_channel: &mut Option<(Sender<AppTx>, Receiver<AppRx>)>, universalis_query_target: &str) {
        let (sender, _) = network_channel.as_mut().unwrap();
        sender.blocking_send(AppTx::RequestItem { item_id: self.item_id, region_datacenter_or_server: universalis_query_target.to_string() }).unwrap();
    }
}

impl ItemData {
    fn new(item_id: ItemId) -> Self {
        Self {
            item_id,
            hq_only: false,
            state: ItemWindowDataState::Loading,
        }
    }
}

#[derive(Deserialize, Serialize, Debug)]
pub(crate) enum ItemWindowButtonState {
    Current,
    History,
}

#[derive(Deserialize, Serialize, Debug)]
pub(crate) enum ItemWindowDataState {
    Loading,
    Loaded {
        button_state: ItemWindowButtonState,
        item_data: MarketView,
        #[serde(skip)]
        query_view: Vec<ListingView>
    },
    Error {
        error: Error,
    },
}

impl ItemWindowDataState {
    pub(crate) fn accept_data(
        &mut self,
        market_data: core::result::Result<MarketView, universalis::Error>,
    ) {
        *self = match market_data {
            Ok(market_data) => ItemWindowDataState::Loaded {
                button_state: ItemWindowButtonState::Current,
                item_data: market_data,
                query_view: vec![]
            },
            Err(e) => ItemWindowDataState::Error {
                error: Error::new(&e),
            },
        }
    }
}

#[derive(Deserialize, Serialize, Default, Debug)]
pub(crate) struct WindowsList {
    pub(crate) recipe_windows: Vec<RecipePriceList>,
    pub(crate) item_windows: Vec<ItemData>,
}

#[derive(Debug)]
pub enum AppTx {
    RequestItem {
        item_id: ItemId,
        region_datacenter_or_server: String,
    },
    RequestRecipe {
        recipe_id: RecipeId,
        region_datacenter_or_server: String,
    },
}

#[derive(Debug)]
pub enum AppRx {
    RecipeResponse {
        recipe_id: RecipeId,
        raw_data: core::result::Result<RecipePricingRawData, universalis::Error>,
    },
    ItemResponse {
        item_id: ItemId,
        market_view: core::result::Result<MarketView, universalis::Error>,
        // todo history:
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

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[derive(Deserialize, Serialize, Debug)]
#[serde(default)] // if we add new fields, give them default values when deserializing old state
pub struct CraftersToolbox {
    windows: WindowsList,
    /// Allows us to communicate with our network thread
    #[serde(skip)]
    network_channel: Option<(Sender<AppTx>, Receiver<AppRx>)>,
    /// Stores user profile information
    user_data: UserData,
    /// Universalis query target. Can be either a region, datacenter, or server.
    /// Potentially can refactor this to use the Universalis types to prevent misuse.
    universalis_query_target: String,
    /// Holds data about datacenters and worlds & what items Universalis can lookup
    #[serde(skip)]
    universalis_data: UniversalisData,
    sidebar_state: SidePanel,
    /// Reference to the static xiv_gen data containing all items & recipes.
    #[serde(skip)]
    game_data: &'static xiv_gen::Data,
}

impl Default for CraftersToolbox {
    fn default() -> Self {
        Self {
            windows: Default::default(),
            network_channel: None,
            user_data: Default::default(),
            universalis_query_target: "".to_string(),
            game_data: CraftersToolbox::decompress_data(),
            universalis_data: UniversalisData::default(),
            sidebar_state: SidePanel::ItemLookup(ItemPanel::new()),
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

        let universalis_data = match network_channel.1.blocking_recv().unwrap() {
            AppRx::UniversalisData { universalis_data } => universalis_data,
            _ => panic!("Expected universalis data"),
        };

        if let Some(storage) = cc.storage {
            let mut value: CraftersToolbox =
                eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();
            value.windows.recipe_windows = value
                .windows
                .recipe_windows
                .into_iter()
                .filter(|e| e.data.is_some())
                .map(|mut m| {
                    m.update_data(&value.game_data);
                    m
                })
                .collect();
            value.windows.item_windows.iter_mut().for_each(|i| i.update_query());
            value.network_channel = Some(network_channel);
            value.universalis_data = universalis_data;
            return value;
        }

        Self {
            network_channel: Some(network_channel),
            universalis_data,
            ..Default::default()
        }
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
}

fn add_disabled_button(ui: &mut egui::Ui, target: &mut String, src: &str) {
    ui.scope(|ui| {
        ui.set_enabled(*target != src);
        if ui.button(src).clicked() {
            *target = src.to_string();
        }
    });
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
            universalis_data,
            windows,
            user_data,
            universalis_query_target,
            sidebar_state,
            game_data,
        } = self;
        let UserData {
            region,
            data_center,
            home_world,
            crafters,
        } = user_data;
        // ICU
        let provider = icu_testdata::get_provider();
        let mut fixed_decimal = FixedDecimalFormatterOptions::default();
        fixed_decimal.grouping_strategy = GroupingStrategy::Auto;
        let decimal_format = FixedDecimalFormatter::try_new_with_buffer_provider(
            &provider,
            &locale!("en").into(),
            fixed_decimal,
        )
        .unwrap();

        if let Some((_, rx)) = network_channel {
            if let Ok(value) = rx.try_recv() {
                match value {
                    AppRx::RecipeResponse {
                        recipe_id,
                        raw_data,
                    } => {
                        if let Some(value) = windows
                            .recipe_windows
                            .iter_mut()
                            .find(|window| window.recipe_id == recipe_id)
                        {
                            let raw_data = raw_data.map_err(|e| Error::new(&e));
                            // todo this sucks
                            let pricing_for_item = if let Some(pricing_for_item) =
                                raw_data.as_ref().ok().map(|raw_data| {
                                    raw_data
                                        .create_best_price_summary(&value.pricing_args, game_data)
                                        .map_err(|e| e.into())
                                }) {
                                pricing_for_item
                            } else {
                                Err(anyhow::Error::msg("No raw data to work from"))
                            }
                            .map_err(|e| serde_error::Error::new(&*e));
                            let world_by_item_pricing = pricing_for_item
                                .as_ref()
                                .map(|m| m.get_items_by_world_cloned())
                                .unwrap_or_default();
                            let datacenter_pricing = raw_data
                                .as_ref()
                                .map_err(|e| Error::new(&*e))
                                .and_then(|m| {
                                    m.get_recipe_target_item_listing_summary()
                                        .map_err(|e| Error::new(&e))
                                });
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
                    AppRx::ItemResponse {
                        item_id,
                        market_view,
                    } => {
                        if let Some(i) = windows
                            .item_windows
                            .iter_mut()
                            .find(|i| i.item_id == item_id)
                        {
                            i.state.accept_data(market_view);
                            i.update_query();
                        } else {
                            warn!("No window for item response {item_id:?}");
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
                    ui.set_min_width(200.0);
                    ui.set_min_height(300.0);
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
                    ui.menu_button(&format!("marketboard filter: {}", universalis_query_target.as_str()), |ui| {
                        add_disabled_button(ui, universalis_query_target, &region.0);
                        if let Some(data_center) = data_center {
                            add_disabled_button(ui, universalis_query_target, &data_center.0);
                            if let Some(worlds) = universalis_data.data_centers.get(data_center) {
                                for world in worlds {
                                    add_disabled_button(ui, universalis_query_target, &world.0);
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
                sidebar_state.draw(ui, universalis_query_target, windows, network_channel, game_data);

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
        let mut remove_item_window = None;
        let mut open_recipe_window = None;
        for (i, item_window) in windows.item_windows.iter_mut().enumerate() {
            let items = game_data.get_items();
            let item = items.get(&item_window.item_id).expect(&format!(
                "item missing from static data {:?}",
                item_window.item_id
            ));
            let item_name = item.get_name();
            egui::Window::new(&format!("{item_name} Pricing"))
                .default_width(400.0)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("❌").clicked() {
                            remove_item_window = Some(i);
                        }
                        if ui.button("🔃").clicked() {
                            item_window.refresh(network_channel, universalis_query_target);
                        }
                        if ui.checkbox(&mut item_window.hq_only, "hq only").changed() {
                            item_window.update_query();
                        }
                        // Check if there's a recipe for this item
                        let recipes = game_data.get_recipes();
                        if let Some(recipe_id) = recipes.iter().find(|(recipe_id, recipe)| recipe.get_item_result() == item_window.item_id).map(|(recipe_id, _)| recipe_id) {
                            ui.scope(|ui| {
                                ui.set_enabled(!windows.recipe_windows.iter().any(|recipe| recipe.recipe_id == *recipe_id));
                                if ui.button("⚒").clicked() {
                                    open_recipe_window = Some(*recipe_id);
                                }
                            });
                        }
                    });
                    match &item_window.state {
                        ItemWindowDataState::Loading => {
                            ui.spinner();
                        }
                        ItemWindowDataState::Loaded {
                            button_state,
                            item_data,
                            query_view,
                        } => match button_state {
                            ItemWindowButtonState::Current => match item_data {
                                MarketView::SingleView(s) => {
                                    ScrollArea::vertical().max_height(400.0).show_rows(ui, 15.0, query_view.len(), |ui, range| {
                                        Grid::new(format!("{:?}", item_window.item_id))
                                            .striped(true)
                                            .num_columns(4)
                                            .show(ui, |ui| {
                                                ui.label("world name");
                                                ui.label("price per unit");
                                                ui.label("quantity");
                                                ui.label("hq");
                                                ui.label("total");
                                                ui.end_row();
                                                for i in range {
                                                    let listing = &query_view[i];
                                                    ui.label(
                                                        listing
                                                            .world_name
                                                            .as_ref()
                                                            .unwrap_or(&"".to_string()),
                                                    );
                                                    ui.label(
                                                        listing
                                                            .price_per_unit
                                                            .unwrap_or_default()
                                                            .to_string(),
                                                    );
                                                    ui.label(
                                                        listing
                                                            .quantity
                                                            .unwrap_or_default()
                                                            .to_string(),
                                                    );
                                                    ui.label(listing.hq.then_some("✅").unwrap_or_default());
                                                    ui.label(listing.total.to_string());
                                                    ui.end_row();
                                                }
                                            });
                                    });
                                }
                                MarketView::MultiView(_) => {}
                            },
                            ItemWindowButtonState::History => {}
                        },
                        ItemWindowDataState::Error { error } => {
                            ui.label(format!("{error}"));
                        }
                    }
                });
        }
        if let Some(recipe_id) = open_recipe_window {
            windows.add_recipe(recipe_id, network_channel, game_data, universalis_query_target.as_str());
        }
        if let Some(remove) = remove_item_window {
            windows.item_windows.remove(remove);
        }

        let mut remove_id = None;
        let mut delayed_open_item_window = None;
        for (i, buddy) in windows.recipe_windows.iter_mut().enumerate() {
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
                            buddy.refresh(network_channel, universalis_query_target.to_string());
                        }
                        if ui.button("❌").clicked() {
                            remove_id = Some(i)
                        }
                    });
                    if let Some(data) = &buddy.data {
                        let world_map = &data.world_by_item_pricing;
                        ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                            Grid::new(buddy.recipe_id)
                                .num_columns(5)
                                .spacing([15.0, 5.0])
                                .striped(true)
                                .show(ui, |ui| {
                                    let item_color = Color32::from_rgb(250, 50, 100);
                                    ui.colored_label(item_color, "item");
                                    ui.colored_label(item_color, "HQ");
                                    ui.colored_label(item_color, "quantity");
                                    ui.colored_label(item_color, "price per item");
                                    ui.colored_label(item_color, "retainer");
                                    ui.end_row();
                                    for (world, items) in world_map {
                                        let world_color = Color32::from_rgb(69, 199, 19);
                                        // ui.colored_label(world_color, "World: ");
                                        ui.colored_label(world_color, world);
                                        ui.end_row();
                                        for (item, listings) in items {
                                            for listing in listings {
                                                ui.label(&item.name);
                                                ui.label(match listing.hq {
                                                    true => "✅",
                                                    false => "",
                                                });
                                                ui.label(listing.quantity.unwrap_or(0).to_string());
                                                ui.label(
                                                    listing
                                                        .price_per_unit
                                                        .map(|price| {
                                                            decimal_format
                                                                .format(&price.into())
                                                                .write_to_string()
                                                                .to_string()
                                                        })
                                                        .unwrap_or_default(),
                                                );
                                                ui.label(&listing.retainer_name);
                                                ui.end_row();
                                            }
                                        }
                                    }
                                });
                        });
                        if let Some(pricing) = draw_err(&data.pricing_for_item, ui) {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 5.0;
                                ui.label("Ingredient cost total: ");

                                ui.label(
                                    (decimal_format
                                        .format(&pricing.total.into())
                                        .write_to_string()
                                        .borrow()) as &str,
                                );
                                ui.label("items total: ");
                                ui.label(pricing.items.len().to_string());
                                if buddy.pricing_args.craft_quantity > 1 {
                                    ui.label("per item: ");
                                    let quantity =
                                        pricing.total / buddy.pricing_args.craft_quantity;
                                    ui.label(
                                        (decimal_format
                                            .format(&quantity.into())
                                            .write_to_string()
                                            .borrow())
                                            as &str,
                                    );
                                }
                            });
                            ui.horizontal(|ui| {
                                if let Some(pricing_summary) = &data.datacenter_pricing {
                                    ui.spacing_mut().item_spacing.x = 5.0;
                                    ui.label(universalis_query_target.as_str());
                                    if let Some(lq) = &pricing_summary.lq_items {
                                        ui.label("LQ: ");
                                        ui.label(lq.to_string());
                                    }
                                    if let Some(hq) = &pricing_summary.hq_items {
                                        ui.label("HQ: ");
                                        ui.label(hq.to_string());
                                    }
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
                                        ui.with_layout(egui::Layout::right_to_left(), |ui| {
                                            let item_id = recipe.get_item_result();
                                            ui.set_enabled(!windows.item_windows.iter().any(|i| i.item_id == item_id));
                                            if ui.button("💲").context_menu(|ui| {
                                                ui.label("Show listings of target item");
                                            }).clicked() {
                                                delayed_open_item_window = Some(item_id);
                                            }
                                        });
                                    });
                                }
                            });
                        }
                    } else {
                        ui.spinner();
                    }
                });
        }
        if let Some(delay) = delayed_open_item_window {
            windows.add_item(delay, network_channel, &universalis_query_target);
        }
        if let Some(remove_id) = remove_id {
            windows.recipe_windows.remove(remove_id);
        }
    }

    /// Called by the frame work to save state before shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }
}

impl WindowsList {
    pub(crate) fn add_recipe(
        &mut self,
        recipe_id: RecipeId,
        network_channel: &mut Option<(Sender<AppTx>, Receiver<AppRx>)>,
        data: &xiv_gen::Data,
        data_center: impl ToString,
    ) {
        if self.recipe_windows.iter().any(|r| r.recipe_id == recipe_id) {
            info!("Duplicate recipe window requested {recipe_id:?}");
            return;
        }
        // let recipe = data.get_recipes().get(&recipe_id).unwrap();
        if let Some((tx, _)) = network_channel {
            tx.blocking_send(AppTx::RequestRecipe {
                recipe_id: recipe_id,
                region_datacenter_or_server: data_center.to_string(),
            })
            .unwrap();
        }
        let pricing_buddy = RecipePriceList {
            recipe_id: recipe_id,
            data: None,
            pricing_args: PricingArguments::default(),
        };
        self.recipe_windows.push(pricing_buddy);
    }

    pub(crate) fn add_item(
        &mut self,
        item_id: ItemId,
        network_channel: &mut Option<(Sender<AppTx>, Receiver<AppRx>)>,
        data_center: impl ToString,
    ) {
        if self.item_windows.iter().any(|i| i.item_id == item_id) {
            info!("Duplciate item id window requested {item_id:?}");
            return;
        }
        let (sender, _receiver) = network_channel
            .as_mut()
            .expect("Tried to do network request without network");
        sender
            .blocking_send(AppTx::RequestItem {
                item_id,
                region_datacenter_or_server: data_center.to_string(),
            })
            .unwrap();
        self.item_windows.push(ItemData::new(item_id));
    }
}
