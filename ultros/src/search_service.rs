use std::sync::Arc;
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, Query, QueryParser};
use tantivy::schema::{STORED, Schema, TextOptions, Value};
use tantivy::{Index, IndexReader, ReloadPolicy, doc};
use tracing::{error, info, warn};
use ultros_api_types::search::SearchResult;
use xiv_gen::{ItemId, ItemSearchCategoryId, ItemUiCategoryId};

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
        let collector = TopDocs::with_limit(10).order_by_score();
        let top_docs = match searcher.search(&query, &collector) {
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
