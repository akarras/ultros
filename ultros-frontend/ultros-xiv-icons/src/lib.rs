use std::{
    collections::HashMap,
    io::{Cursor, Read},
};

use once_cell::sync::OnceCell;
use tar::Archive;
use ultros_api_types::icon_size::IconSize;

/// Icon images keyed by *icon* id, plus the item id → icon id map. Icons are
/// stored once per icon id because thousands of items share an icon; the
/// `items.map` tar entry (ascii `<item id> <icon id>` lines) translates.
struct IconData {
    item_to_icon: HashMap<i32, i32>,
    images: HashMap<(i32, IconSize), Vec<u8>>,
}

fn parse_image_name(str: &str) -> (i32, IconSize) {
    // <icon id>_<size>.webp
    let (name, _ext) = str.split_once('.').unwrap();
    let (id, size) = name.split_once('_').unwrap();
    (
        id.parse().unwrap(),
        match size {
            "Large" => IconSize::Large,
            "Medium" => IconSize::Medium,
            _ => panic!("Size did not match any known string? {}", size),
        },
    )
}

fn parse_item_map(contents: &str) -> HashMap<i32, i32> {
    contents
        .lines()
        .map(|line| {
            let (item, icon) = line.split_once(' ').expect("items.map line");
            (
                item.parse().expect("items.map item id"),
                icon.parse().expect("items.map icon id"),
            )
        })
        .collect()
}

fn icon_data() -> &'static IconData {
    let tar = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/icons/images.tar.zst"
    ))
    .as_ref();
    static DATA: OnceCell<IconData> = OnceCell::new();
    DATA.get_or_init(|| {
        let mut decoder = zstd::Decoder::new(tar).unwrap();
        let mut data = vec![];
        decoder.read_to_end(&mut data).unwrap();
        let mut archive = Archive::new(Cursor::new(data));
        let mut item_to_icon = HashMap::new();
        let mut images = HashMap::new();
        for entry in archive.entries_with_seek().unwrap().flatten() {
            let mut bytes = vec![];
            let mut entry = entry;
            entry.read_to_end(&mut bytes).unwrap();
            let name = entry.path().unwrap().display().to_string();
            if name == "items.map" {
                item_to_icon = parse_item_map(std::str::from_utf8(&bytes).unwrap());
            } else {
                images.insert(parse_image_name(&name), bytes);
            }
        }
        assert!(
            !item_to_icon.is_empty(),
            "icon pack has no items.map entry — data/icons/images.tar.zst predates the \
             icon-id-keyed format; regenerate it with game-data-pack"
        );
        IconData {
            item_to_icon,
            images,
        }
    })
}

/// Bytes of the packed WebP for `item_id` at `image_size`.
///
/// The pack only stores Large (80px — the native 2x resolution) and Medium
/// (40px) — a dedicated Small encode saved almost nothing over the 40px WebP
/// while adding 50% more entries, so Small requests are served the Medium
/// bytes and the browser scales them down to the 25px display box.
pub fn get_item_image(item_id: i32, image_size: IconSize) -> Option<&'static [u8]> {
    let image_size = match image_size {
        IconSize::Small => IconSize::Medium,
        other => other,
    };
    let data = icon_data();
    let icon_id = data.item_to_icon.get(&item_id)?;
    data.images
        .get(&(*icon_id, image_size))
        .map(|v| v.as_slice())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn currencies_have_icons() {
        // The old universalis-assets source only covered marketable items, so
        // every currency rendered blank. Gil and Allagan Tomestone of Poetics
        // must exist now that the pack is extracted from the game files.
        for item_id in [1, 28] {
            assert!(
                get_item_image(item_id, IconSize::Large).is_some(),
                "item {item_id} has no Large icon"
            );
        }
    }

    #[test]
    fn small_requests_serve_the_medium_bytes() {
        // The pack stores Large and Medium only; Small must alias Medium
        // rather than returning None.
        let small = get_item_image(5057, IconSize::Small).expect("small icon");
        let medium = get_item_image(5057, IconSize::Medium).expect("medium icon");
        assert!(std::ptr::eq(small, medium));
    }

    #[test]
    fn items_sharing_an_icon_share_the_bytes() {
        // Iron Ingot (5057) and its HQ-less siblings aside, the cheapest
        // guarantee: two different item ids mapped to the same icon id return
        // the same slice, proving the pack stores one copy per icon.
        let data = icon_data();
        let mut by_icon: HashMap<i32, i32> = HashMap::new();
        let (a, b) = data
            .item_to_icon
            .iter()
            .find_map(|(item, icon)| by_icon.insert(*icon, *item).map(|earlier| (earlier, *item)))
            .expect("some two items share an icon");
        let bytes_a = get_item_image(a, IconSize::Large).expect("icon a");
        let bytes_b = get_item_image(b, IconSize::Large).expect("icon b");
        assert!(std::ptr::eq(bytes_a, bytes_b));
    }
}
