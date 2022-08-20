use std::collections::HashMap;
use xiv_gen::RecipeId;
use crate::CraftersToolbox;
use crate::crafting_types::CraftJob;

/// Enum for the side panel
#[derive(Serialize, Deserialize, Debug)]
pub(crate) enum SidePanel {
    ItemLookup {

    },
    RecipeLookup {
        #[serde(skip)]
        recipes: Vec<(RecipeId, String, Vec<CraftJob>)>,
        /// Holds the query results for the previous recipe query
        #[serde(skip)]
        recipe_query_results: Vec<(RecipeId, String, Vec<CraftJob>)>,
        /// Represents the users current query
        recipe_query: String,
    }
}

impl SidePanel {
    pub fn draw(&mut self, ui: &mut egui::Ui, game_data: &xiv_gen::Data) {
        match self {
            SidePanel::ItemLookup { .. } => {}
            SidePanel::RecipeLookup { .. } => {}
        }
    }

    pub(crate) fn new_recipe_lookup() -> Self {
        let recipes = Self::create_recipe_list();

        SidePanel::RecipeLookup {
            recipes: recipes.clone(),
            recipe_query_results: recipes,
            recipe_query: "".to_string()
        }
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

    /// Prepares all the recipe data we need for recipes
    fn create_recipe_list() -> Vec<(RecipeId, String, Vec<CraftJob>)> {
        // this might be good to store somewhere
        let game_data = CraftersToolbox::decompress_data();
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



}

