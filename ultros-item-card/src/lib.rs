//! Deterministic social-preview cards for Ultros item pages.
//!
//! These images deliberately contain no market values or timestamps. Social
//! crawlers cache previews independently of the site, so the card is a stable
//! breadcrumb to the live item page rather than a stale market snapshot.

use std::sync::OnceLock;

use anyhow::{Context, Result, anyhow};
use ril::prelude::{
    Ellipse, Font, Image as RilImage, ImageFormat, LinearGradient, OverlayMode, Polygon, Rectangle,
    ResizeAlgorithm, Rgba, TextLayout, TextSegment, WrapStyle,
};
use ultros_api_types::icon_size::IconSize;

pub const WIDTH: u32 = 1200;
pub const HEIGHT: u32 = 630;

const RENDER_SCALE: u32 = 2;
const CANVAS_WIDTH: u32 = WIDTH * RENDER_SCALE;
const CANVAS_HEIGHT: u32 = HEIGHT * RENDER_SCALE;

const JALDI_REGULAR: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../ultros/static/Jaldi-Regular.ttf"
));
const JALDI_BOLD: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../ultros/static/Jaldi-Bold.ttf"
));

struct CardFonts {
    regular: Font,
    bold: Font,
}

fn card_fonts() -> Result<&'static CardFonts> {
    static FONTS: OnceLock<Result<CardFonts, String>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            Ok(CardFonts {
                regular: Font::from_bytes(JALDI_REGULAR, 96.0).map_err(|e| e.to_string())?,
                bold: Font::from_bytes(JALDI_BOLD, 112.0).map_err(|e| e.to_string())?,
            })
        })
        .as_ref()
        .map_err(|error| anyhow!("failed to load embedded item-card fonts: {error}"))
}

const fn rgba(hex: u32, alpha: u8) -> Rgba {
    Rgba::new(
        ((hex >> 16) & 0xff) as u8,
        ((hex >> 8) & 0xff) as u8,
        (hex & 0xff) as u8,
        alpha,
    )
}

fn draw_rounded_rect(
    image: &mut RilImage<Rgba>,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    radius: u32,
    fill: Rgba,
) {
    let radius = radius.min(width / 2).min(height / 2);
    image.draw(
        &Rectangle::from_bounding_box(x + radius, y, x + width - radius, y + height)
            .with_fill(fill),
    );
    image.draw(
        &Rectangle::from_bounding_box(x, y + radius, x + width, y + height - radius)
            .with_fill(fill),
    );
    for (left, top) in [
        (x, y),
        (x + width - 2 * radius, y),
        (x, y + height - 2 * radius),
        (x + width - 2 * radius, y + height - 2 * radius),
    ] {
        image.draw(
            &Ellipse::from_bounding_box(left, top, left + 2 * radius, top + 2 * radius)
                .with_fill(fill),
        );
    }
}

fn draw_rounded_panel(
    image: &mut RilImage<Rgba>,
    bounds: (u32, u32, u32, u32),
    radius: u32,
    border_width: u32,
    border: Rgba,
    fill: Rgba,
) {
    let (x, y, width, height) = bounds;
    draw_rounded_rect(image, x, y, width, height, radius, border);
    draw_rounded_rect(
        image,
        x + border_width,
        y + border_width,
        width - 2 * border_width,
        height - 2 * border_width,
        radius.saturating_sub(border_width),
        fill,
    );
}

fn draw_sparkle(image: &mut RilImage<Rgba>, center: (u32, u32), radius: u32, fill: Rgba) {
    let (x, y) = center;
    let narrow = radius / 4;
    image.draw(
        &Polygon::from_vertices([
            (x, y - radius),
            (x + narrow, y - narrow),
            (x + radius, y),
            (x + narrow, y + narrow),
            (x, y + radius),
            (x - narrow, y + narrow),
            (x - radius, y),
            (x - narrow, y - narrow),
        ])
        .with_fill(fill)
        .with_antialiased(true),
    );
}

fn item_icon(item_id: i32) -> Result<Option<RilImage<Rgba>>> {
    let Some(bytes) = ultros_xiv_icons::get_item_image(item_id, IconSize::Large) else {
        return Ok(None);
    };
    let decoded = image::load_from_memory_with_format(bytes, image::ImageFormat::WebP)
        .context("failed to decode packed item icon")?
        .to_rgba8();
    let pixels = decoded
        .pixels()
        .map(|pixel| Rgba::new(pixel[0], pixel[1], pixel[2], pixel[3]))
        .collect::<Vec<_>>();
    let mut icon =
        RilImage::from_pixels(decoded.width(), pixels).with_overlay_mode(OverlayMode::Merge);
    icon.resize(360, 360, ResizeAlgorithm::Lanczos3);
    Ok(Some(icon))
}

fn title_layout<'a>(font: &'a Font, title: &str, size: f32) -> TextLayout<'a, Rgba> {
    TextLayout::new()
        .with_position(1000, 420)
        .with_width(800)
        .with_wrap(WrapStyle::Word)
        .with_segment(
            &TextSegment::new(font, title, rgba(0xf7f1e8, 255))
                .with_size(size)
                .with_wrap(WrapStyle::Word),
        )
}

/// Render a 1200×630 PNG social card for one item and market scope.
pub fn render_item_card(item_id: i32, item_name: &str, scope: &str) -> Result<Vec<u8>> {
    let fonts = card_fonts()?;
    let mut card = RilImage::new(CANVAS_WIDTH, CANVAS_HEIGHT, rgba(0x100b14, 255))
        .with_overlay_mode(OverlayMode::Merge);

    let background = LinearGradient::new()
        .with_angle_degrees(18.0)
        .with_start_color(rgba(0x100b14, 255))
        .with_color_at(0.58, rgba(0x190f20, 255))
        .with_end_color(rgba(0x0c0910, 255));
    card.draw(
        &Rectangle::from_bounding_box(0, 0, CANVAS_WIDTH, CANVAS_HEIGHT).with_fill(background),
    );

    draw_rounded_panel(
        &mut card,
        (24, 24, 2352, 1212),
        42,
        5,
        rgba(0x72547d, 230),
        rgba(0x100b14, 255),
    );

    // A quiet purple disc gives the item area depth without implying that any
    // of the card content is live market data.
    card.draw(&Ellipse::circle(420, 720, 200).with_fill(rgba(0x231330, 255)));

    draw_rounded_panel(
        &mut card,
        (110, 300, 760, 760),
        48,
        5,
        rgba(0x64436e, 235),
        rgba(0x160e1b, 255),
    );

    card.draw(
        &Ellipse::circle(490, 680, 275)
            .with_fill(rgba(0x8b5cf6, 18))
            .with_overlay_mode(OverlayMode::Merge),
    );
    if let Some(icon) = item_icon(item_id)? {
        card.paste(310, 500, &icon);
    } else {
        draw_sparkle(&mut card, (490, 680), 78, rgba(0xc084fc, 220));
    }

    card.draw(
        &TextSegment::new(&fonts.bold, "U L T R O S", rgba(0xc084fc, 255))
            .with_position(110, 102)
            .with_size(64.0),
    );

    let scope = scope.replace('-', " ").to_uppercase();
    card.draw(
        &TextSegment::new(
            &fonts.bold,
            format!("MARKET BOARD   /   {scope}"),
            rgba(0xb889dc, 255),
        )
        .with_position(1000, 300)
        .with_size(48.0),
    );

    let mut title_size = 120.0;
    let title = loop {
        let layout = title_layout(&fonts.bold, item_name, title_size);
        if layout.height() <= 320 || title_size <= 76.0 {
            break layout;
        }
        title_size -= 6.0;
    };
    let title_height = title.height();
    card.draw(&title);

    let subtitle_y = (420 + title_height + 42).min(820);
    card.draw(
        &TextSegment::new(
            &fonts.regular,
            "Live listings, prices, and sale history",
            rgba(0xc1b7c2, 255),
        )
        .with_position(1000, subtitle_y)
        .with_size(48.0),
    );

    draw_rounded_panel(
        &mut card,
        (1000, 970, 100, 100),
        50,
        4,
        rgba(0x55365f, 255),
        rgba(0x130d18, 255),
    );
    draw_sparkle(&mut card, (1050, 1020), 29, rgba(0xc084fc, 255));
    card.draw(
        &TextSegment::new(&fonts.regular, "View on ultros.app", rgba(0xd39af1, 255))
            .with_position(1135, 982)
            .with_size(56.0),
    );
    card.draw(&Rectangle::from_bounding_box(1135, 1058, 1585, 1062).with_fill(rgba(0xa56ac5, 210)));

    card.resize(WIDTH, HEIGHT, ResizeAlgorithm::Lanczos3);
    let mut bytes = Vec::new();
    card.encode(ImageFormat::Png, &mut bytes)
        .context("failed to encode item card as PNG")?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_valid_card(bytes: &[u8]) {
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        let image = image::load_from_memory(bytes).expect("generated card is a decodable image");
        assert_eq!(image.width(), WIDTH);
        assert_eq!(image.height(), HEIGHT);
        assert!(
            bytes.len() > 20_000,
            "card unexpectedly lacks visual content"
        );
    }

    #[test]
    fn renders_item_card_with_packed_icon() {
        let bytes = render_item_card(49318, "Courtly Lover's Wristlet of Aiming", "North-America")
            .expect("item card");
        if let Some(path) = std::env::var_os("ULTROS_ITEM_CARD_PREVIEW") {
            std::fs::write(path, &bytes).expect("write requested item-card preview");
        }
        assert_valid_card(&bytes);
    }

    #[test]
    fn renders_long_item_names_without_failing() {
        let bytes = render_item_card(
            -1,
            "Augmented Ceremonial Long Item Name of Absolutely Unreasonable Testing",
            "North-America",
        )
        .expect("long-name item card");
        assert_valid_card(&bytes);
    }
}
