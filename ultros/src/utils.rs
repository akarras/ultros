use xiv_gen::ItemId;

pub(crate) fn get_item_name(item_id: i32) -> &'static str {
    xiv_gen_db::decompress_data()
        .items
        .get(&ItemId(item_id))
        .map(|item| item.name.as_str())
        .unwrap_or_default()
}

pub(crate) fn get_item_icon_url(item_id: i32) -> String {
    format!("https://universalis-ffxiv.github.io/universalis-assets/icon2x/{item_id}.png")
}
