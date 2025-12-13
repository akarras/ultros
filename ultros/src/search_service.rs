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
                return vec![];
            }
        };

        let top_docs = match searcher.search(&query, &TopDocs::with_limit(10)) {
            Ok(docs) => docs,
            Err(e) => {
                error!("SearchService: Search execution failed: {}", e);
                return vec![];
            }
        };

        top_docs
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
            })
            .collect()
    }
}
