use crate::app::WindowsList;
use crate::crafting_types::CraftJob;
use crate::{AppRx, AppTx, CraftersToolbox};
use eframe::emath::Align;
use egui::{ScrollArea, Ui};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::mpsc::{Receiver, Sender};
use xiv_gen::RecipeId;

#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct RecipeSearchPanel {
    #[serde(skip)]
    recipes: Vec<(RecipeId, String, Vec<CraftJob>)>,
    /// Holds the query results for the previous recipe query
    #[serde(skip)]
    recipe_query_results: Vec<(RecipeId, String, Vec<CraftJob>)>,
    /// Represents the users current query
    recipe_query: String,
}

impl RecipeSearchPanel {
    pub fn draw(
        &mut self,
        ui: &mut Ui,
        universalis_datacenter: &str,
        windows: &mut WindowsList,
        network_channel: &mut Option<(Sender<AppTx>, Receiver<AppRx>)>,
        _game_data: &xiv_gen::Data,
    ) {
        self.check_init();
        ui.heading("Recipe Lookup");
        if ui.text_edit_singleline(&mut self.recipe_query).changed() {
            self.update_search();
        }
        let recipe_query_results = &self.recipe_query_results;
        ScrollArea::vertical().show_rows(ui, 15.0, recipe_query_results.len(), |ui, range| {
            for i in range {
                let (id, item_name, jobs) = &recipe_query_results[i];
                ui.horizontal(|ui| {
                    ui.label(item_name.as_str());
                    ui.with_layout(egui::Layout::right_to_left(Align::RIGHT), |ui| {
                        ui.scope(|ui| {
                            let already_open = windows
                                .recipe_windows
                                .iter()
                                .any(|list| *id == list.recipe_id);
                            ui.set_enabled(!already_open);
                            if ui.button("💲").clicked() {
                                windows.add_recipe(
                                    *id,
                                    network_channel,
                                    universalis_datacenter.to_string(),
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
    }

    pub(crate) fn new() -> Self {
        let recipes = Self::create_recipe_list();
        Self {
            recipes: recipes.clone(),
            recipe_query_results: recipes,
            recipe_query: "".to_string(),
        }
    }

    fn check_init(&mut self) {
        if self.recipes.is_empty() {
            self.recipes = Self::create_recipe_list();
            self.update_search();
        }
    }

    fn update_search(&mut self) {
        let Self {
            recipes,
            recipe_query_results,
            recipe_query,
        } = self;
        let lower = recipe_query.to_lowercase();

        *recipe_query_results = recipes
            .iter()
            .filter(|(_, name, _)| name.to_lowercase().contains(&lower))
            .cloned()
            .collect()
    }

    /// Prepares all the recipe data we need for recipes
    fn create_recipe_list() -> Vec<(RecipeId, String, Vec<CraftJob>)> {
        // this might be good to store somewhere
        let game_data = xiv_gen_db::data();
        let recipes = &game_data.recipes;
        let items = &game_data.items;
        let recipe_lookup = &game_data.recipe_lookups;
        let mut jobs: HashMap<RecipeId, Vec<CraftJob>> =
            recipe_lookup
                .values()
                .fold(HashMap::new(), |mut map, lookup| {
                    Self::try_insert_recipe(&mut map, lookup.crp, CraftJob::Carpenter);
                    Self::try_insert_recipe(&mut map, lookup.bsm, CraftJob::Blacksmith);
                    Self::try_insert_recipe(&mut map, lookup.arm, CraftJob::Armorer);
                    Self::try_insert_recipe(&mut map, lookup.gsm, CraftJob::Goldsmith);
                    Self::try_insert_recipe(&mut map, lookup.ltw, CraftJob::Leatherworker);
                    Self::try_insert_recipe(&mut map, lookup.wvr, CraftJob::Weaver);
                    Self::try_insert_recipe(&mut map, lookup.alc, CraftJob::Alchemist);
                    Self::try_insert_recipe(&mut map, lookup.cul, CraftJob::Culinarian);
                    map
                });
        recipes
            .values()
            .map(|r| (r.key_id, r.item_result))
            .filter(|(_id, result)| result.0 != 0)
            .map(|(recipe_id, item_id)| {
                (
                    recipe_id,
                    items
                        .get(&item_id)
                        .unwrap_or_else(|| panic!("unable to get item_id: {}", item_id.0)),
                )
            })
            .map(|(recipe_id, item)| {
                (
                    recipe_id,
                    item.name.clone(),
                    jobs.remove(&recipe_id).unwrap_or_default(),
                )
            })
            .collect()
    }

    /// small utility function
    fn try_insert_recipe(
        map: &mut HashMap<RecipeId, Vec<CraftJob>>,
        recipe_id: RecipeId,
        crafter: CraftJob,
    ) {
        if recipe_id.0 == 0 {
            return;
        }
        map.entry(recipe_id).or_default().push(crafter);
    }
}
