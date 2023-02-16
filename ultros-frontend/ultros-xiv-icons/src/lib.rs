use std::io::{Cursor, Read};

use tar::Archive;
use ultros_api_types::icon_size::IconSize;

pub fn get_item_image(item_id: i32, image_size: IconSize) -> Option<Vec<u8>> {
    let tar = include_bytes!(concat!(env!("OUT_DIR"), "/images.tar")).as_ref();
    // somehow this is slower than converting images in real time- figure it out ;-;
    let mut archive = Archive::new(Cursor::new(tar));
    let file = format!("{item_id}_{image_size}.webp");
    let mut data = vec![];
    let entry = archive
        .entries_with_seek()
        .ok()?
        .flatten()
        .find(|entry| {
            entry
                .path()
                .ok()
                .and_then(|path| path.as_os_str().to_str().map(|str| str.to_string()))
                .map(|str| str.contains(&file))
                .unwrap_or_default()
        })
        .map(|mut entry| entry.read_to_end(&mut data))?
        .ok()?;
    if entry == 0 {
        return None;
    }
    Some(data)
}
