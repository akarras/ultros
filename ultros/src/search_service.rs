use std::collections::BTreeMap;
use std::sync::Arc;
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, Query, QueryParser};
use tantivy::schema::{STORED, Schema, TextOptions, Value};
use tantivy::{Index, IndexReader, ReloadPolicy, doc};
use tracing::{error, info, warn};
use ultros_api_types::search::SearchResult;
use xiv_gen::{
    ClassJobId, Data, ItemId, ItemSearchCategoryId, ItemUiCategoryId, Recipe, RecipeId,
    RecipeLevelTableId,
};

#[derive(Clone)]
pub struct SearchService {
    index: Arc<Index>,
    reader: IndexReader,
    title_field: tantivy::schema::Field,
    type_field: tantivy::schema::Field,
    url_field: tantivy::schema::Field,
    icon_id_field: tantivy::schema::Field,
    category_field: tantivy::schema::Field,
    display_category_field: tantivy::schema::Field,
}

/// Documents scored before weighting. The user sees the best
/// [`SEARCH_RESULTS`] of the weighted list, so this has to be wide enough that
/// an item demoted below a recipe by the raw scores can still climb back.
const SEARCH_CANDIDATES: usize = 30;

/// Results returned to the search box.
const SEARCH_RESULTS: usize = 10;

/// Score multiplier applied to a result by its type.
///
/// Every craftable item is indexed twice: once as its item page, once as its
/// recipe (issue #1384). For a marketable item the item page is what people
/// mean, so a recipe never outranks an equally good item match. It still wins
/// when it is the better match — or the only one, which is the point for the
/// untradeable results the item index skips.
fn type_weight(result_type: &str) -> f32 {
    match result_type {
        "recipe" => 0.5,
        _ => 1.0,
    }
}

/// Applies [`type_weight`] to raw tantivy scores and keeps the best
/// [`SEARCH_RESULTS`].
///
/// The sort is stable, so results that weigh the same stay in the order
/// tantivy ranked them.
fn rank(mut results: Vec<SearchResult>) -> Vec<SearchResult> {
    for result in &mut results {
        result.score *= type_weight(&result.result_type);
    }
    results.sort_by(|a, b| b.score.total_cmp(&a.score));
    results.truncate(SEARCH_RESULTS);
    results
}

/// Row id of the carpenter `ClassJob`, the first of the eight Disciples of the
/// Hand.
fn carpenter_row(data: &Data) -> Option<i32> {
    data.class_jobs
        .iter()
        .find(|(_, job)| job.abbreviation == "CRP")
        .map(|(id, _)| id.0)
}

/// Crafter and level shown under a recipe result, e.g. `Carpenter Lv. 60`.
///
/// `Recipe::craft_type` is a row index into the `CraftType` sheet, which
/// xiv-gen doesn't load. The eight crafter `ClassJob` rows are consecutive
/// from carpenter in the same order, which the recipe analyzer pins against
/// real game data in `craft_type_acronyms_match_the_crafter_class_jobs`.
fn recipe_label(data: &Data, carpenter: Option<i32>, recipe: &Recipe) -> String {
    let job = carpenter
        .filter(|_| (0..8).contains(&recipe.craft_type))
        .and_then(|carpenter| {
            data.class_jobs
                .get(&ClassJobId(carpenter + recipe.craft_type))
        })
        .map(|job| job.name.as_str())
        .unwrap_or("Recipe");
    match data
        .recipe_level_tables
        .get(&RecipeLevelTableId(recipe.recipe_level_table))
        .map(|level| level.class_job_level)
    {
        Some(level) if level > 0 => format!("{job} Lv. {level}"),
        _ => job.to_string(),
    }
}

/// Reduces what the user typed to the plain text the index was built from.
///
/// The search box is an item-name search, not a query language, so `Lover's`
/// has to find "Courtly Lover's Scepter". tantivy's `QueryParser` gives `'`,
/// `"`, `:`, `(`, `^`, `*`, `[` and the uppercase words `AND`/`OR`/`NOT`/`IN`
/// a meaning of their own, and an unbalanced quote is a hard parse error that
/// `search` turns into zero results (issue #1298).
///
/// Titles were indexed with `en_stem`, whose `SimpleTokenizer` splits on every
/// non-alphanumeric char and then lowercases, so applying the same two rules
/// here yields exactly the tokens the index holds while leaving the parser
/// nothing to misread.
fn plain_text_query(query: &str) -> String {
    query
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect()
}

impl SearchService {
    pub fn new() -> anyhow::Result<Self> {
        let mut schema_builder = Schema::builder();

        // Use a tokenizer that handles apostrophes better if possible, or just standard English
        // For now, we'll stick to standard but rely on fuzzy search to help with "Samurai's" vs "Samurai"
        let title_options = TextOptions::default()
            .set_indexing_options(
                tantivy::schema::TextFieldIndexing::default()
                    .set_tokenizer("en_stem")
                    .set_index_option(tantivy::schema::IndexRecordOption::WithFreqsAndPositions),
            )
            .set_stored();

        let title_field = schema_builder.add_text_field("title", title_options.clone());
        let type_field = schema_builder.add_text_field("type", STORED);
        let url_field = schema_builder.add_text_field("url", STORED);
        let icon_id_field = schema_builder.add_i64_field("icon_id", STORED);
        // Category field uses same options as title for searchability
        let category_field = schema_builder.add_text_field("category", title_options);
        // Shown under the title, never searched. A recipe's crafter lives here
        // rather than in `category` so that "carpenter" keeps returning the
        // carpenter gear page instead of a thousand carpenter recipes.
        let display_category_field = schema_builder.add_text_field("display_category", STORED);

        let schema = schema_builder.build();

        let index = Index::create_in_ram(schema.clone());
        let mut index_writer = index.writer(50_000_000)?;

        let data = xiv_gen_db::data();

        // Index Items
        for (id, item) in &data.items {
            if item.item_search_category > 0 {
                let category_name = data
                    .item_search_categorys
                    .get(&ItemSearchCategoryId(item.item_search_category))
                    .map(|c| c.name.as_str())
                    .unwrap_or("");

                index_writer.add_document(doc!(
                    title_field => item.name.as_str(),
                    type_field => "item",
                    url_field => format!("/item/{}", id.0),
                    icon_id_field => id.0 as i64, // Use Item ID for image lookup
                    category_field => category_name,
                    display_category_field => category_name,
                ))?;
            }
        }

        // Index Categories
        for cat in data.item_search_categorys.values() {
            index_writer.add_document(doc!(
                title_field => cat.name.as_str(),
                type_field => "category",
                // Keyed by id: this index is built from English game data, so
                // a name-keyed URL would send every non-English visitor to a
                // category their client cannot resolve. See
                // `resolve_category_param` in `item_explorer.rs`.
                url_field => format!("/items/category/{}", cat.key_id.0),
                // Categories don't have a direct icon, maybe use a default or 0
                icon_id_field => 0i64,
                category_field => "",
                display_category_field => "",
            ))?;
        }

        // Index Jobs
        for job in data.class_jobs.values() {
            if job.job_index > 0 || job.doh_dol_job_index >= 0 {
                let name = if job.abbreviation.is_empty() {
                    job.name.to_string()
                } else {
                    format!("{} ({})", job.name, job.abbreviation)
                };
                index_writer.add_document(doc!(
                    title_field => name,
                    type_field => "job equipment", // Renamed from "job"
                    url_field => format!("/items/jobset/{}", job.name),
                    icon_id_field => 0i64, // Jobs don't have a simple icon ID in this context easily accessible or needed?
                    category_field => "",
                    display_category_field => "",
                ))?;
            }
        }

        // Index Currencies
        // Logic adapted from CurrencySelection to find items used as currency for marketable items
        let ui_categories = &data.item_ui_categorys;
        let allowed_item_ui_categories = ["Currency", "Miscellany", "Other"]
            .into_iter()
            .filter_map(|category| {
                ui_categories
                    .iter()
                    .find(|f| f.1.name == category)
                    .map(|(id, _)| *id)
            })
            .collect::<Vec<_>>();

        let mut currency_ids = std::collections::HashSet::new();

        // Extract currency items from special shops
        // The SpecialShop struct now has a flat `item: Vec<u16>` containing all item IDs.
        // We check if any items in the shop are marketable, and if so, collect all non-zero
        // item IDs as potential currencies (filtered later by UI category).
        for shop in data.special_shops.values() {
            let has_marketable_item = shop.item.iter().any(|&item_id| {
                item_id != 0
                    && data
                        .items
                        .get(&ItemId(item_id as i32))
                        .is_some_and(|item| item.item_search_category > 0)
            });

            if has_marketable_item {
                for &item_id in &shop.item {
                    if item_id != 0 {
                        currency_ids.insert(ItemId(item_id as i32));
                    }
                }
            }
        }

        for item in data.items.values() {
            if item.name == "Gil" || item.name == "MGP" {
                currency_ids.insert(item.key_id);
            }
        }

        for id in currency_ids {
            if let Some(item) = data.items.get(&id)
                && (allowed_item_ui_categories.contains(&ItemUiCategoryId(item.item_ui_category))
                    || item.name == "Gil"
                    || item.name == "MGP")
            {
                index_writer.add_document(doc!(
                    title_field => item.name.as_str(),
                    type_field => "currency",
                    url_field => format!("/currency-exchange/{}", id.0),
                    icon_id_field => id.0 as i64, // Use Item ID for image lookup
                    category_field => "",
                    display_category_field => "",
                ))?;
            }
        }

        // Index Recipes
        //
        // The item index only covers marketable items, so a craft whose result
        // is untradeable — rarefied collectables, quest turn-ins — had no way
        // into the search box even when every ingredient is bought on the
        // market (issue #1384). Recipes are indexed for every craftable item,
        // untradeable or not, since the planner is worth reaching directly;
        // `type_weight` keeps them under the item page.
        //
        // Several recipes can share a result item (cross-class crafts). Index
        // the lowest recipe id of each so the list holds one row per craftable
        // item rather than the same name repeated per job.
        let mut recipe_by_result: BTreeMap<i32, RecipeId> = BTreeMap::new();
        for (id, recipe) in &data.recipes {
            if recipe.item_result == 0 {
                continue;
            }
            let first = recipe_by_result.entry(recipe.item_result).or_insert(*id);
            if id.0 < first.0 {
                *first = *id;
            }
        }

        let carpenter = carpenter_row(data);
        for (item_id, recipe_id) in recipe_by_result {
            let (Some(item), Some(recipe)) = (
                data.items.get(&ItemId(item_id)),
                data.recipes.get(&recipe_id),
            ) else {
                continue;
            };
            if item.name.is_empty() {
                continue;
            }

            index_writer.add_document(doc!(
                title_field => item.name.as_str(),
                type_field => "recipe",
                url_field => format!("/recipe/{}", recipe_id.0),
                icon_id_field => item_id as i64, // Use Item ID for image lookup
                // Left unsearchable on purpose: the recipe is found by the name
                // of what it makes, the same string the item document holds.
                category_field => "",
                display_category_field => recipe_label(data, carpenter, recipe),
            ))?;
        }

        index_writer.commit()?;
        info!("SearchService: Indexing complete.");

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;

        Ok(Self {
            index: Arc::new(index),
            reader,
            title_field,
            type_field,
            url_field,
            icon_id_field,
            category_field,
            display_category_field,
        })
    }

    pub fn search(&self, query_str: &str) -> Vec<SearchResult> {
        let searcher = self.reader.searcher();
        // Exact match parser (High boost)
        let mut exact_parser =
            QueryParser::for_index(&self.index, vec![self.title_field, self.category_field]);
        exact_parser.set_field_boost(self.title_field, 5.0);
        exact_parser.set_field_boost(self.category_field, 1.0);

        // Fuzzy match parser (Low boost)
        let mut fuzzy_parser =
            QueryParser::for_index(&self.index, vec![self.title_field, self.category_field]);
        fuzzy_parser.set_field_boost(self.title_field, 0.5);
        fuzzy_parser.set_field_boost(self.category_field, 0.1);
        fuzzy_parser.set_field_fuzzy(self.title_field, false, 2, true);
        fuzzy_parser.set_field_fuzzy(self.category_field, false, 1, true);

        let query_str = plain_text_query(query_str);
        let exact_query = exact_parser.parse_query(&query_str);
        let fuzzy_query = fuzzy_parser.parse_query(&query_str);

        let query = match (exact_query, fuzzy_query) {
            (Ok(eq), Ok(fq)) => Box::new(BooleanQuery::union(vec![eq, fq])) as Box<dyn Query>,
            (Ok(eq), Err(_)) => eq,
            (Err(_), Ok(fq)) => fq,
            (Err(e), Err(_)) => {
                warn!("SearchService: Invalid query '{}': {}", query_str, e);
                return vec![];
            }
        };

        // tantivy 0.26: `TopDocs` itself no longer implements `Collector`; chain
        // `.order_by_score()` to get a score-ordered collector (the previous default).
        // More candidates than the box shows: `rank` reweights by result type
        // afterwards, so the final ten aren't tantivy's top ten.
        let collector = TopDocs::with_limit(SEARCH_CANDIDATES).order_by_score();
        let top_docs = match searcher.search(&query, &collector) {
            Ok(docs) => docs,
            Err(e) => {
                error!("SearchService: Search execution failed: {}", e);
                return vec![];
            }
        };

        let results = top_docs
            .into_iter()
            .map(|(score, doc_address)| {
                let retrieved_doc: tantivy::schema::TantivyDocument =
                    searcher.doc(doc_address).unwrap();
                let title = retrieved_doc
                    .get_first(self.title_field)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let result_type = retrieved_doc
                    .get_first(self.type_field)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let url = retrieved_doc
                    .get_first(self.url_field)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let icon_id = retrieved_doc
                    .get_first(self.icon_id_field)
                    .and_then(|v| v.as_i64())
                    .map(|v| v as i32);
                let category = retrieved_doc
                    .get_first(self.display_category_field)
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                SearchResult {
                    score,
                    title,
                    result_type,
                    url,
                    icon_id,
                    category,
                }
            })
            .collect();

        rank(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_query_strips_query_syntax_the_index_never_saw() {
        // Every char the tokenizer split on becomes a separator, so the query
        // yields exactly the tokens the index holds.
        assert_eq!(
            plain_text_query("courtly lover's scepter"),
            "courtly lover s scepter"
        );
        assert_eq!(
            plain_text_query("Skybuilders' Alembic"),
            "skybuilders  alembic"
        );
        assert_eq!(
            plain_text_query("title:foo (bar) \"baz\" -qux^2 ["),
            "title foo  bar   baz   qux 2  "
        );
        // Uppercase operators are lowercased into ordinary words.
        assert_eq!(
            plain_text_query("Ring AND Thing OR NOT IN"),
            "ring and thing or not in"
        );
    }

    /// Issue #1298: typing the item's real name, apostrophe included, returned
    /// nothing because tantivy's grammar treats `'` as a phrase delimiter and
    /// an unbalanced one is a parse error.
    #[test]
    fn finds_an_item_whose_name_has_an_apostrophe() {
        let service = SearchService::new().expect("index builds from embedded data");
        let data = xiv_gen_db::data();
        let item = data
            .items
            .values()
            .find(|i| i.item_search_category > 0 && i.name.contains("'s "))
            .expect("game data has a marketable possessive item name");

        let query = item.name.to_lowercase();
        let results = service.search(&query);
        assert!(
            results.iter().any(|r| r.title == item.name),
            "searching {query:?} did not return {:?}, got {:?}",
            item.name,
            results.iter().map(|r| &r.title).collect::<Vec<_>>()
        );
    }

    fn result(result_type: &str, score: f32) -> SearchResult {
        SearchResult {
            score,
            title: "Iron Ingot".to_string(),
            result_type: result_type.to_string(),
            url: format!("/{result_type}"),
            icon_id: None,
            category: None,
        }
    }

    #[test]
    fn a_recipe_never_outranks_an_equally_good_item_match() {
        // Both documents hold the same title, so tantivy scores them alike;
        // the type weight is what decides the order.
        let ranked = rank(vec![result("recipe", 10.0), result("item", 10.0)]);
        assert_eq!(
            ranked.iter().map(|r| &r.result_type).collect::<Vec<_>>(),
            ["item", "recipe"]
        );
    }

    #[test]
    fn a_much_better_recipe_match_still_ranks_first() {
        let ranked = rank(vec![result("item", 10.0), result("recipe", 30.0)]);
        assert_eq!(
            ranked.first().map(|r| r.result_type.as_str()),
            Some("recipe")
        );
    }

    #[test]
    fn rank_returns_at_most_a_boxful() {
        let ranked = rank((0..50).map(|i| result("item", i as f32)).collect());
        assert_eq!(ranked.len(), SEARCH_RESULTS);
        assert_eq!(ranked.first().map(|r| r.score), Some(49.0));
    }

    /// Issue #1384: a craft whose result can't be sold has no item page in the
    /// index, so before recipes were indexed there was no way to search for it
    /// even though its ingredients are bought on the market.
    #[test]
    fn finds_a_craft_whose_result_is_not_marketable() {
        let service = SearchService::new().expect("index builds from embedded data");
        let data = xiv_gen_db::data();

        let mut untradeable: Vec<_> = data
            .recipes
            .values()
            .filter_map(|recipe| {
                let item = data.items.get(&ItemId(recipe.item_result))?;
                (item.item_search_category == 0 && !item.name.is_empty()).then_some(&item.name)
            })
            .collect();
        untradeable.sort();
        untradeable.dedup();
        assert!(
            !untradeable.is_empty(),
            "game data should have untradeable craft results"
        );

        // A sample rather than every one of them: this builds a real index and
        // runs a real query per name.
        for name in untradeable.iter().take(20) {
            let results = service.search(name);
            assert!(
                results
                    .iter()
                    .any(|r| r.result_type == "recipe" && &&r.title == name),
                "searching {name:?} returned no recipe, got {:?}",
                results
                    .iter()
                    .map(|r| (&r.title, &r.result_type))
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn a_marketable_craft_lists_its_item_page_above_its_recipe() {
        let service = SearchService::new().expect("index builds from embedded data");
        let data = xiv_gen_db::data();
        let item = data
            .recipes
            .values()
            .filter_map(|recipe| data.items.get(&ItemId(recipe.item_result)))
            .find(|item| item.item_search_category > 0 && !item.name.is_empty())
            .expect("game data has a marketable craft");

        let results = service.search(&item.name);
        let position = |result_type: &str| {
            results
                .iter()
                .position(|r| r.title == item.name && r.result_type == result_type)
        };
        let (Some(item_at), Some(recipe_at)) = (position("item"), position("recipe")) else {
            panic!(
                "expected both an item and a recipe for {:?}, got {:?}",
                item.name,
                results
                    .iter()
                    .map(|r| (&r.title, &r.result_type))
                    .collect::<Vec<_>>()
            );
        };
        assert!(
            item_at < recipe_at,
            "{:?} listed its recipe ({recipe_at}) above its item page ({item_at})",
            item.name
        );
    }

    #[test]
    fn a_recipe_result_carries_the_crafter_as_its_category() {
        let data = xiv_gen_db::data();
        let carpenter = carpenter_row(data);
        let recipe = data
            .recipes
            .values()
            .find(|r| r.craft_type == 0)
            .expect("game data has a carpenter recipe");
        assert!(
            recipe_label(data, carpenter, recipe).starts_with("Carpenter"),
            "unexpected label {:?}",
            recipe_label(data, carpenter, recipe)
        );
    }

    #[test]
    fn query_syntax_characters_never_produce_a_parse_failure() {
        let service = SearchService::new().expect("index builds from embedded data");
        for query in ["lover's", "(", "\"", "title:", "[", "*", "AND", "'"] {
            // Must not panic; results may legitimately be empty.
            let _ = service.search(query);
        }
        assert!(!service.search("lover's").is_empty());
    }
}
