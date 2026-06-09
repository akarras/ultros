//! Item icons for SVG embedding. The source assets are WebP, which resvg
//! can't decode — transcode to PNG and inline as a data URI.

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use ultros_api_types::icon_size::IconSize;

/// Item icon as a `data:image/png;base64,…` URI, or `None` if there is no
/// icon for this item.
pub fn item_icon_data_uri(item_id: i32) -> Option<String> {
    let webp = ultros_xiv_icons::get_item_image(item_id, IconSize::Medium)?;
    encode_png_data_uri(webp)
}

fn encode_png_data_uri(webp: &[u8]) -> Option<String> {
    let decoded = image::load_from_memory_with_format(webp, image::ImageFormat::WebP).ok()?;
    let mut png = Vec::new();
    decoded
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(format!("data:image/png;base64,{}", STANDARD.encode(&png)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_webp_to_png_data_uri() {
        let img = image::DynamicImage::new_rgb8(4, 4);
        let mut webp = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut webp), image::ImageFormat::WebP)
            .unwrap();
        let uri = encode_png_data_uri(&webp).unwrap();
        assert!(uri.starts_with("data:image/png;base64,"));
    }
}
