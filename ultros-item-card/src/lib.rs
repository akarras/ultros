//! Deterministic, localized social previews for Ultros.
//!
//! Callers supply evergreen, translated copy. The renderer embeds its fonts and
//! artwork, never reads market data, and never depends on a host font install.

mod fonts;
mod text;

use std::{io::Cursor, sync::OnceLock};

use anyhow::{Context, Result, anyhow};
use fontdue::{Font, FontSettings};
use image::{Rgba, RgbaImage, imageops};
use ultros_api_types::icon_size::IconSize;

pub const WIDTH: u32 = 1200;
pub const HEIGHT: u32 = 630;
const SCALE: u32 = 2;
const IVORY: Rgba<u8> = Rgba([247, 241, 232, 255]);
const LAVENDER: Rgba<u8> = Rgba([192, 123, 238, 255]);
const MUTED: Rgba<u8> = Rgba([195, 179, 211, 255]);

/// Locale controls regional glyph forms; all copy is supplied by the caller.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CardLocale {
    #[default]
    En,
    Ja,
    De,
    Fr,
    Ko,
    Cn,
    Tc,
}

impl CardLocale {
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "en" => Some(Self::En),
            "ja" => Some(Self::Ja),
            "de" => Some(Self::De),
            "fr" => Some(Self::Fr),
            "ko" => Some(Self::Ko),
            "cn" => Some(Self::Cn),
            "tc" => Some(Self::Tc),
            _ => None,
        }
    }
}

/// The source asset to place over the shared violet glow.
#[derive(Clone, Copy, Debug)]
pub enum CardHero {
    Item(i32),
    /// A codepoint from the site's existing FFXIVAppIcons job font.
    Job(char),
    Currency,
    Search,
    Analyzer,
    Help,
}

/// Stable, localized content for a 1200 by 630 social preview.
pub struct CardContent<'a> {
    pub title: &'a str,
    pub subtitle: &'a str,
    pub eyebrow: &'a str,
    pub footer: &'a str,
    pub hero: CardHero,
    pub locale: CardLocale,
}

/// Render the shared card template. This does no networking or filesystem IO.
pub fn render_card(content: &CardContent<'_>) -> Result<Vec<u8>> {
    let fonts = fonts::for_locale(content.locale)?;
    let latin = fonts::for_locale(CardLocale::En)?;
    let mut card = background();

    text::draw_line(
        &mut card,
        &latin.bold,
        "Ultros",
        58.0,
        (60.0, 44.0),
        LAVENDER,
    );
    let eyebrow = text::fit(&fonts.bold, content.eyebrow, 22.0, 15.0, 710.0, 30.0, 1);
    text::draw_fitted(
        &mut card,
        &fonts.bold,
        &eyebrow,
        (1140.0 - eyebrow.width, 62.0),
        LAVENDER,
    );

    let title = text::fit(&fonts.bold, content.title, 96.0, 38.0, 730.0, 302.0, 4);
    // Short headlines sit closer to the hero's centre; three-line headlines
    // retain the approved template's large left-aligned silhouette.
    let title_y = 163.0 + (285.0 - title.height).max(0.0) * 0.32;
    text::draw_fitted(&mut card, &fonts.bold, &title, (60.0, title_y), IVORY);
    let subtitle = text::fit(&fonts.regular, content.subtitle, 29.0, 20.0, 755.0, 64.0, 2);
    text::draw_fitted(&mut card, &fonts.regular, &subtitle, (60.0, 479.0), MUTED);

    draw_hero(&mut card, content.hero)?;
    for x in 60 * SCALE..1140 * SCALE {
        for y in 548 * SCALE..549 * SCALE {
            text::blend_pixel(&mut card, x as i32, y as i32, Rgba([174, 124, 204, 140]));
        }
    }
    let footer = text::fit(&fonts.regular, content.footer, 25.0, 17.0, 815.0, 36.0, 1);
    text::draw_fitted(&mut card, &fonts.regular, &footer, (60.0, 575.0), LAVENDER);
    let domain_width = text::width(&latin.bold, "ultros.app", 26.0);
    text::draw_line(
        &mut card,
        &latin.bold,
        "ultros.app",
        26.0,
        (1140.0 - domain_width, 575.0),
        LAVENDER,
    );

    let card = imageops::resize(&card, WIDTH, HEIGHT, imageops::FilterType::Lanczos3);
    let mut bytes = Cursor::new(Vec::new());
    card.write_to(&mut bytes, image::ImageFormat::Png)
        .context("failed to encode social card")?;
    Ok(bytes.into_inner())
}

/// Compatibility wrapper for existing English item-card callers.
pub fn render_item_card(item_id: i32, item_name: &str, scope: &str) -> Result<Vec<u8>> {
    render_card(&CardContent {
        title: item_name,
        subtitle: "Compare listings across worlds",
        eyebrow: "FFXIV MARKET BOARD",
        footer: &scope.replace('-', " "),
        hero: CardHero::Item(item_id),
        locale: CardLocale::En,
    })
}

fn background() -> RgbaImage {
    // Analytic falloff avoids rings/banding from stacked translucent ellipses.
    // Two overlapping glows put the brightest violet just below the artwork.
    RgbaImage::from_fn(WIDTH * SCALE, HEIGHT * SCALE, |x, y| {
        let x = x as f32 / SCALE as f32;
        let y = y as f32 / SCALE as f32;
        let broad = (-((x - 975.0) / 186.0).powi(2) - ((y - 317.0) / 167.0).powi(2)).exp();
        let core = (-((x - 971.0) / 114.0).powi(2) - ((y - 383.0) / 105.0).powi(2)).exp();
        Rgba([
            (14.0 + 32.0 * broad + 45.0 * core) as u8,
            (9.0 + 13.0 * broad + 17.0 * core) as u8,
            (19.0 + 51.0 * broad + 81.0 * core) as u8,
            255,
        ])
    })
}

fn paste_icon(card: &mut RgbaImage, icon: &RgbaImage, center: (u32, u32), size: u32) {
    let target = size * SCALE;
    let ratio = target as f32 / icon.width().max(icon.height()) as f32;
    let resized = imageops::resize(
        icon,
        (icon.width() as f32 * ratio).round().max(1.0) as u32,
        (icon.height() as f32 * ratio).round().max(1.0) as u32,
        imageops::FilterType::Lanczos3,
    );
    imageops::overlay(
        card,
        &resized,
        i64::from(center.0 * SCALE) - i64::from(resized.width() / 2),
        i64::from(center.1 * SCALE) - i64::from(resized.height() / 2),
    );
}

fn draw_hero(card: &mut RgbaImage, hero: CardHero) -> Result<()> {
    match hero {
        CardHero::Item(id) => {
            if let Some(bytes) = ultros_xiv_icons::get_item_image(id, IconSize::Large) {
                let icon = image::load_from_memory_with_format(bytes, image::ImageFormat::WebP)
                    .context("failed to decode packed item icon")?
                    .into_rgba8();
                paste_icon(card, &icon, (970, 328), 208);
            } else {
                draw_symbol(card, icondata_bi::BiSearchAlt2Regular, (970, 328), 225)?;
            }
        }
        CardHero::Job(glyph) => {
            static FONT: OnceLock<Result<Font, String>> = OnceLock::new();
            let font = FONT
                .get_or_init(|| {
                    Font::from_bytes(
                        include_bytes!(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/../ultros/static/classjob-icons/src/FFXIVAppIcons.ttf"
                        )) as &[u8],
                        FontSettings {
                            scale: 512.0,
                            ..FontSettings::default()
                        },
                    )
                    .map_err(str::to_owned)
                })
                .as_ref()
                .map_err(|error| anyhow!("failed to load job font: {error}"))?;
            if font.lookup_glyph_index(glyph) == 0 {
                draw_symbol(card, icondata_bi::BiSearchAlt2Regular, (970, 328), 225)?;
            } else {
                let (metrics, bitmap) = font.rasterize(glyph, 260.0 * SCALE as f32);
                if metrics.width == 0 || metrics.height == 0 {
                    return draw_symbol(card, icondata_bi::BiSearchAlt2Regular, (970, 328), 225);
                }
                let icon =
                    RgbaImage::from_fn(metrics.width as u32, metrics.height as u32, |x, y| {
                        Rgba([
                            247,
                            230,
                            189,
                            bitmap[y as usize * metrics.width + x as usize],
                        ])
                    });
                paste_icon(card, &icon, (970, 328), 235);
            }
        }
        CardHero::Currency => {
            let gil = image::load_from_memory(include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../ultros/static/images/gil.png"
            )))?
            .into_rgba8();
            paste_icon(card, &gil, (949, 305), 211);
            draw_symbol(card, icondata_bi::BiTransferAltRegular, (1038, 406), 108)?;
        }
        CardHero::Search => draw_symbol(card, icondata_bi::BiSearchAlt2Regular, (970, 328), 245)?,
        CardHero::Analyzer => {
            draw_symbol(card, icondata_bi::BiBarChartAlt2Regular, (970, 328), 245)?
        }
        CardHero::Help => draw_symbol(card, icondata_bi::BiHelpCircleRegular, (970, 328), 245)?,
    }
    Ok(())
}

fn draw_symbol(
    card: &mut RgbaImage,
    icon: icondata_core::Icon,
    center: (u32, u32),
    size: u32,
) -> Result<()> {
    // These are the same Boxicons SVG paths used by the site's icon component.
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="{}" fill="#edddff">{}</svg>"##,
        icon.view_box.unwrap_or("0 0 24 24"),
        icon.data,
    );
    let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default())
        .context("failed to parse embedded social-card icon")?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size * SCALE, size * SCALE)
        .context("failed to allocate social-card icon")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(SCALE as f32, SCALE as f32),
        &mut pixmap.as_mut(),
    );
    // tiny-skia returns premultiplied RGBA; image::overlay expects straight RGBA.
    let pixels = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let pixel = pixel.demultiply();
            [pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()]
        })
        .collect();
    let icon = RgbaImage::from_raw(size * SCALE, size * SCALE, pixels)
        .context("invalid rendered social-card icon dimensions")?;
    paste_icon(card, &icon, center, size);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_valid_card(bytes: &[u8]) {
        let image = image::load_from_memory(bytes).expect("decodable generated PNG");
        assert_eq!((image.width(), image.height()), (WIDTH, HEIGHT));
        assert!(bytes.len() > 20_000);
    }

    #[test]
    fn renders_packed_item_and_missing_icon() {
        for id in [49318, -1] {
            let bytes = render_item_card(id, "Courtly Lover’s Wristlet of Aiming", "North-America")
                .expect("item card");
            assert_valid_card(&bytes);
        }
    }

    #[test]
    fn renders_every_hero_including_missing_job() {
        for hero in [
            CardHero::Job('\u{f034}'),
            CardHero::Job('\0'),
            CardHero::Currency,
            CardHero::Search,
            CardHero::Analyzer,
            CardHero::Help,
        ] {
            let bytes = render_card(&CardContent {
                title: "Your next market board advantage.",
                subtitle: "Compare prices. Plan your purchases.",
                eyebrow: "FINAL FANTASY XIV",
                footer: "Final Fantasy XIV",
                hero,
                locale: CardLocale::En,
            })
            .expect("tool card");
            assert_valid_card(&bytes);
        }
    }

    #[test]
    fn output_is_deterministic() {
        let render = || render_item_card(-1, "Currency Exchange", "Final Fantasy XIV").unwrap();
        assert_eq!(render(), render());
    }
}
