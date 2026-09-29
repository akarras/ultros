//! Generate deterministic-shaped browser fixtures (peer ids may vary).
//! cargo run -p ultros-list-doc --example compatibility_fixtures
//! Redirect stdout to integration/fixtures/list-compatibility.json.
use ultros_list_doc::{ListDocument, RowKey};

fn main() {
    let supported = ListDocument::new();
    supported.rename("Compatibility fixture").unwrap();
    supported.add_row(RowKey::new(5056, None), 5, None).unwrap();
    let future = ListDocument::from_snapshot(&supported.export_snapshot().unwrap()).unwrap();
    future
        .inner()
        .get_map("meta")
        .insert("schema", 999_i64)
        .unwrap();
    future.commit();
    let malformed = ListDocument::from_snapshot(&supported.export_snapshot().unwrap()).unwrap();
    malformed
        .inner()
        .get_map("rows")
        .insert("5057:any", "not a row map")
        .unwrap();
    malformed.commit();
    println!(
        "{{\"supported\":{:?},\"future\":{:?},\"malformed\":{:?}}}",
        supported.export_snapshot().unwrap(),
        future.export_snapshot().unwrap(),
        malformed.export_snapshot().unwrap()
    );
}
