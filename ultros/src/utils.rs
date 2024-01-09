use xiv_gen::ItemId;

pub(crate) fn get_item_name(item_id: i32) -> &'static str {
    xiv_gen_db::data()
        .items
        .get(&ItemId(item_id))
        .map(|item| item.name.as_str())
        .unwrap_or_default()
}
