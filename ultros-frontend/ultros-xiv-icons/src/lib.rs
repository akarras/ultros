use std::{
    collections::HashMap,
    io::{Cursor, Read},
};

use flate2::read::GzDecoder;
use once_cell::sync::OnceCell;
use tar::Archive;
use ultros_api_types::icon_size::IconSize;

fn parse_url(str: &str) -> (i32, IconSize) {
    // id_size.webp
    let (name, _ext) = str.split_once('.').unwrap();
    // id size
    let (id, size) = name.split_once('_').unwrap();
    (
        id.parse().unwrap(),
        match size {
            "Large" => IconSize::Large,
            "Medium" => IconSize::Medium,
            "Small" => IconSize::Small,
            _ => panic!("Size did not match any known string? {}", size),
        },
    )
}

pub fn get_item_image(item_id: i32, image_size: IconSize) -> Option<&'static [u8]> {
    let tar = include_bytes!(concat!(env!("OUT_DIR"), "/images.tar.gz")).as_ref();
    static IMAGES: OnceCell<HashMap<(i32, IconSize), Vec<u8>>> = OnceCell::new();
    // Dump all of our images into a static hashmap
    let data = IMAGES.get_or_init(|| {
        let mut decoder = GzDecoder::new(tar);
        let mut data = vec![];
        decoder.read_to_end(&mut data).unwrap();
        let mut archive = Archive::new(Cursor::new(data));
        let entries: HashMap<_, _> = archive
            .entries_with_seek()
            .ok()
            .unwrap()
            .flatten()
            .map(|mut entry| {
                let mut bytes = vec![];
                entry.read_to_end(&mut bytes).unwrap();
                entry
                    .path()
                    .ok()
                    .and_then(|path| path.as_os_str().to_str().map(|str| parse_url(str)))
                    .map(|key| (key, bytes))
                    .unwrap()
            })
            .collect();
        entries
    });

    data.get(&(item_id, image_size)).map(|v| v.as_slice())
}
