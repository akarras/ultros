use anyhow::Result;
use lazy_static::lazy_static;
use tantivy::collector::TopDocs;
use tantivy::doc;
use tantivy::query::QueryParser;
use tantivy::schema::*;
use tantivy::Index;
use xiv_gen::Item;

fn build_item_search_schema() -> Result<Index> {
    let mut schema_builder = Schema::builder();
    let id_field = schema_builder.add_i64_field("id", STORED);
    let name_field = schema_builder.add_text_field("name", TEXT);
    let category_field = schema_builder.add_text_field("category", TEXT);
    let schema = schema_builder.build();
    let ram = Index::create_in_ram(schema);
    let mut index_writer = ram.writer(10000000)?;
    let documents = xiv_gen_db::decompress_data();
    let items = &documents.items;
    let categories = &documents.item_ui_categorys;
    // should also filter on marketable items
    for (id, item) in items {
        let category = categories
            .get(&item.item_ui_category)
            .map(|m| m.name.to_string())
            .unwrap_or_default();
        index_writer.add_document(doc!(
          id_field => id.0 as i64,
          name_field => item.name.clone(),
          category_field => category,
        ))?;
    }
    index_writer.commit()?;
    Ok(ram)
}

pub fn do_query(query_str: &str) -> Result<Vec<(i32, &'static Item)>> {
    lazy_static! {
        static ref ITEM_INDEX: Index = build_item_search_schema().unwrap();
    };
    let name = ITEM_INDEX
        .schema()
        .get_field("name")
        .ok_or(anyhow::Error::msg("Unable to get name field"))?;
    let category = ITEM_INDEX
        .schema()
        .get_field("category")
        .ok_or(anyhow::Error::msg("Unable to get category field"))?;
    let parser = QueryParser::for_index(&ITEM_INDEX, vec![name, category]);
    let query = parser.parse_query(query_str).unwrap();
    let name = ITEM_INDEX.schema().get_field("name").unwrap();
    let category = ITEM_INDEX.schema().get_field("category").unwrap();
    let reader = ITEM_INDEX.reader().unwrap();
    let searcher = reader.searcher();
    let results = searcher.search(&query, &TopDocs::with_limit(10))?;
    let items = &xiv_gen_db::decompress_data().items;
    results
        .iter()
        .map(|(_, item)| {
            let doc = searcher.doc(*item)?;
            let item_id = doc.field_values()[0]
                .value
                .as_i64()
                .ok_or(anyhow::Error::msg("Unexpected field data type"))?
                as i32;
            let item = xiv_gen::ItemId(item_id);
            let item = items
                .get(&item)
                .ok_or(anyhow::Error::msg("Item not found"))?;
            Ok((item_id, item))
        })
        .collect()
}

#[cfg(test)]
mod test {
    use tantivy::{collector::TopDocs, query::QueryParser};

    use super::{build_item_search_schema, do_query};

    #[test]
    fn test_build_item_schema() {
        let index = build_item_search_schema().unwrap();
        let name = index.schema().get_field("name").unwrap();
        let category = index.schema().get_field("category").unwrap();
        let reader = index.reader().unwrap();
        let searcher = reader.searcher();
        let parser = QueryParser::for_index(&index, vec![name, category]);
        let query = parser.parse_query("indoor marble fountain").unwrap();
        let top_docs = searcher.search(&query, &TopDocs::with_limit(10)).unwrap();
        let (_score, doc) = top_docs.first().unwrap();
        let doc = searcher.doc(*doc).unwrap();
        let value = &doc.field_values()[0];
        let item_id = value.value.as_i64().unwrap() as i32;
        assert_eq!(item_id, 37349);
    }

    #[test]
    fn test_simple_query() {
        let fountain = do_query("indoor marble fountain").unwrap();
        assert_eq!(fountain[0].1.name, "Indoor Marble Fountain");
        let ingot = do_query("bronze ingot").unwrap();
        assert_eq!(ingot[0].0, 5056)
    }
}
