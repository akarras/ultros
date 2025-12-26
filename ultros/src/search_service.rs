use std::sync::Arc;
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, Query, QueryParser};
use tantivy::schema::{STORED, Schema, TextOptions, Value};
use tantivy::{Index, IndexReader, ReloadPolicy, doc};
use tracing::{error, info, warn};
use ultros_api_types::search::SearchResult;

#[derive(Clone)]
pub struct SearchService {
    index: Arc<Index>,
    reader: IndexReader,
    title_field: tantivy::schema::Field,
    type_field: tantivy::schema::Field,
    url_field: tantivy::schema::Field,
    icon_id_field: tantivy::schema::Field,
    category_field: tantivy::schema::Field,
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

        let schema = schema_builder.build();

        let index = Index::create_in_ram(schema.clone());
        let mut index_writer = index.writer(50_000_000)?;

        let data = xiv_gen_db::data();

        // Index Items
        for (id, item) in &data.items {
            if item.item_search_category.0 > 0 {
                let category_name = data
                    .item_search_categorys
                    .get(&item.item_search_category)
                    .map(|c| c.name.as_str())
                    .unwrap_or("");

                index_writer.add_document(doc!(
                    title_field => item.name.as_str(),
                    type_field => "item",
                    url_field => format!("/item/{}", id.0),
                    icon_id_field => id.0 as i64, // Use Item ID for image lookup
                    category_field => category_name,
                ))?;
            }
        }

        // Index Categories
        for cat in data.item_search_categorys.values() {
            index_writer.add_document(doc!(
                title_field => cat.name.as_str(),
                type_field => "category",
                url_field => format!("/items/category/{}", cat.name),
                // Categories don't have a direct icon, maybe use a default or 0
                icon_id_field => 0i64,
                category_field => "",
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

        // Helper to extract cost items from special shop
        for shop in data.special_shops.values() {
            let mut has_marketable_reward = false;
            for item_id in shop.item_receive_0.iter().chain(shop.item_receive_1.iter()) {
                if let Some(item) = data.items.get(item_id)
                    && item.item_search_category.0 > 0
                {
                    has_marketable_reward = true;
                    break;
                }
            }

            if has_marketable_reward {
                for item_id in shop
                    .item_cost_0
                    .iter()
                    .chain(shop.item_cost_1.iter())
                    .chain(shop.item_cost_2.iter())
                {
                    if item_id.0 != 0 {
                        currency_ids.insert(item_id);
                    }
                }
            }
        }

        for item in data.items.values() {
            if item.name == "Gil" || item.name == "MGP" {
                currency_ids.insert(&item.key_id);
            }
        }

        for id in currency_ids {
            if let Some(item) = data.items.get(id)
                && (allowed_item_ui_categories.contains(&item.item_ui_category)
                    || item.name == "Gil"
                    || item.name == "MGP")
            {
                index_writer.add_document(doc!(
                    title_field => item.name.as_str(),
                    type_field => "currency",
                    url_field => format!("/currency-exchange/{}", id.0),
                    icon_id_field => id.0 as i64, // Use Item ID for image lookup
                    category_field => "",
                ))?;
            }
        }

        // Index Leves
        // Only iterate Crafting Leves as they are the primary focus for analyzers
        for craft_leve in data.craft_leves.values() {
            if let Some(leve) = data.leves.get(&craft_leve.leve) {
                // Link to the turn-in item's page, which shows Leve info
                let turn_in_item_id = craft_leve.item_0.0;
                if turn_in_item_id > 0 {
                    index_writer.add_document(doc!(
                        title_field => leve.name.as_str(),
                        type_field => "Leve",
                        url_field => format!("/item/{}", turn_in_item_id),
                        icon_id_field => turn_in_item_id as i64,
                        category_field => "Levequest",
                    ))?;
                }
            }
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
        })
    }

    pub fn search(&self, query_str: &str) -> Vec<SearchResult> {
        let mut results = Vec::new();
        let data = xiv_gen_db::data();

        // Check for direct Item ID match
        if let Ok(id) = query_str.trim().parse::<i32>() {
            if let Some(item) = data.items.get(&xiv_gen::ItemId(id)) {
                if item.item_search_category.0 > 0 {
                    let category_name = data
                        .item_search_categorys
                        .get(&item.item_search_category)
                        .map(|c| c.name.as_str())
                        .unwrap_or("")
                        .to_string();

                    results.push(SearchResult {
                        score: 1000.0, // High score for direct match
                        title: format!("{} (ID: {})", item.name, id),
                        result_type: "item".to_string(),
                        url: format!("/item/{}", id),
                        icon_id: Some(id),
                        category: Some(category_name),
                    });
                }
            }
        }

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

        let exact_query = exact_parser.parse_query(query_str);
        let fuzzy_query = fuzzy_parser.parse_query(query_str);

        let query = match (exact_query, fuzzy_query) {
            (Ok(eq), Ok(fq)) => Box::new(BooleanQuery::union(vec![eq, fq])) as Box<dyn Query>,
            (Ok(eq), Err(_)) => eq,
            (Err(_), Ok(fq)) => fq,
            (Err(e), Err(_)) => {
                warn!("SearchService: Invalid query '{}': {}", query_str, e);
                return results; // Return any ID matches if query was invalid for tantivy but valid ID (unlikely if numeric)
            }
        };

        let top_docs = match searcher.search(&query, &TopDocs::with_limit(10)) {
            Ok(docs) => docs,
            Err(e) => {
                error!("SearchService: Search execution failed: {}", e);
                return results;
            }
        };

        results.extend(top_docs
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
                    .get_first(self.category_field)
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
            }));

        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_service_initialization() {
        if let Ok(_service) = SearchService::new() {
             assert!(true);
        } else {
            panic!("Failed to init service");
        }
    }

    #[test]
    fn test_search_by_id() {
        let service = SearchService::new().expect("Init failed");
        let data = xiv_gen_db::data();

        // Find a marketable item
        if let Some((id, item)) = data.items.iter().find(|(_, i)| i.item_search_category.0 > 0) {
            let id_str = id.0.to_string();
            let results = service.search(&id_str);
            if results.is_empty() {
                 panic!("No results found for ID {} ({})", id_str, item.name);
            }
            let first = &results[0];
            assert_eq!(first.result_type, "item");
            assert!(first.title.contains(&id_str));
        } else {
            panic!("No marketable items found in DB to test with");
        }
    }

    #[test]
    fn test_search_leve() {
        let service = SearchService::new().expect("Init failed");
        let data = xiv_gen_db::data();

        // Find a craft leve that links to a valid Leve
        if let Some(craft_leve) = data.craft_leves.values().find(|cl| data.leves.contains_key(&cl.leve) && cl.item_0.0 > 0) {
             let leve = data.leves.get(&craft_leve.leve).unwrap();
             let name = &leve.name;

             let results = service.search(name);
             let found = results.iter().find(|r| r.result_type == "Leve");

             if found.is_none() {
                 let titles: Vec<_> = results.iter().map(|r| r.title.clone()).collect();
                 panic!("Leve not found for '{}'. Found: {:?}", name, titles);
             }
        } else {
             // If no craft leves, we can't test. But this shouldn't happen in real DB.
             // If minimal DB, maybe.
             if !data.craft_leves.is_empty() {
                  panic!("Craft leves exist but none matched criteria?");
             }
        }
    }
}
