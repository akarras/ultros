//! Measured text with Unicode line-break opportunities and bounded fallbacks.

use image::{Rgba, RgbaImage};
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

use crate::SCALE;
use crate::fonts::Face as Font;

pub(crate) struct FittedText {
    pub lines: Vec<String>,
    pub size: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Default)]
struct Bounds {
    left: f32,
    right: f32,
    above: f32,
    below: f32,
}

fn bounds(font: &Font, text: &str, size: f32) -> Bounds {
    let mut bounds = Bounds::default();
    let mut cursor = 0.0;
    let mut previous = None;
    for ch in text.chars() {
        if let Some(previous) = previous {
            cursor += font.horizontal_kern(previous, ch, size).unwrap_or(0.0);
        }
        let metrics = font.metrics(ch, size);
        bounds.left = bounds.left.min(cursor + metrics.xmin as f32);
        bounds.right = bounds
            .right
            .max(cursor + metrics.xmin as f32 + metrics.width as f32);
        bounds.above = bounds
            .above
            .max(metrics.ymin as f32 + metrics.height as f32);
        bounds.below = bounds.below.max(-metrics.ymin as f32);
        cursor += metrics.advance_width;
        previous = Some(ch);
    }
    bounds.right = bounds.right.max(cursor);
    bounds
}

pub(crate) fn width(font: &Font, text: &str, size: f32) -> f32 {
    let bounds = bounds(font, text, size);
    bounds.right - bounds.left
}

fn normalized(text: &str) -> String {
    // The supported card languages are LTR. Some upstream Korean item labels
    // contain a stray right-to-left mark; invisible direction controls must
    // never become a visible missing-glyph square in the raster output.
    let composed: String = text
        .nfc()
        .filter(|ch| {
            !matches!(ch,
                '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
            )
        })
        .collect();
    composed.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn wrapped(font: &Font, text: &str, size: f32, max_width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut start = 0;
    for (end, _) in unicode_linebreak::linebreaks(text) {
        // Unicode permits breaks between Hangul syllables, but Korean copy
        // reads more naturally with its space-delimited words kept intact.
        // Oversized single words still use the grapheme-safe fallback below.
        if font.keeps_words_together()
            && end != text.len()
            && !text[..end].ends_with(char::is_whitespace)
        {
            continue;
        }
        let segment = &text[start..end];
        start = end;
        let candidate = format!("{line}{segment}");
        if width(font, candidate.trim_end(), size) <= max_width {
            line = candidate;
            continue;
        }
        if !line.trim().is_empty() {
            lines.push(line.trim().to_owned());
        }
        line = segment.trim_start().to_owned();
        if width(font, line.trim_end(), size) > max_width {
            // Long unspaced identifiers still fit. Never separate a combining
            // mark from its base character, or cut through a UTF-8 codepoint.
            let oversized = std::mem::take(&mut line);
            for grapheme in oversized.graphemes(true) {
                let candidate = format!("{line}{grapheme}");
                if !line.is_empty() && width(font, &candidate, size) > max_width {
                    lines.push(std::mem::take(&mut line));
                }
                line.push_str(grapheme);
            }
        }
    }
    if !line.trim().is_empty() {
        lines.push(line.trim().to_owned());
    }
    lines
}

fn layout(font: &Font, lines: Vec<String>, size: f32) -> FittedText {
    let height = lines.last().map_or(0.0, |last| {
        let last = bounds(font, last, size);
        lines.len().saturating_sub(1) as f32 * size * 1.04 + last.above + last.below
    });
    let width = lines
        .iter()
        .map(|line| width(font, line, size))
        .fold(0.0, f32::max);
    FittedText {
        lines,
        size,
        width,
        height,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn fit(
    font: &Font,
    text: &str,
    max_size: f32,
    min_size: f32,
    max_width: f32,
    max_height: f32,
    max_lines: usize,
) -> FittedText {
    // Compose accents and kana marks before measuring/rasterizing so canonically
    // equivalent localized names produce identical text without mark shaping.
    let normalized = normalized(text);
    // Bounds work for accidentally huge upstream labels while preserving all
    // normal game names. An ellipsis makes any final truncation explicit.
    let mut text: String = normalized.graphemes(true).take(512).collect();
    if text.len() < normalized.len() {
        text.push('…');
    }
    let mut size = max_size;
    loop {
        let mut fitted = layout(font, wrapped(font, &text, size, max_width), size);
        if fitted.lines.len() <= max_lines
            && fitted.height <= max_height
            && fitted.width <= max_width
        {
            // Avoid a tiny orphan such as "Samurai gear / sets". Narrow the
            // candidate measure only for two-line headlines, preserving the
            // approved three-line item composition and Unicode break rules.
            if max_lines > 2 && fitted.lines.len() == 2 {
                let first_width = width(font, &fitted.lines[0], size);
                let last_width = width(font, &fitted.lines[1], size);
                if last_width < first_width * 0.5 {
                    let mut best_difference = first_width - last_width;
                    let mut candidate_width = max_width - 10.0;
                    while candidate_width > max_width * 0.5 {
                        let candidate =
                            layout(font, wrapped(font, &text, size, candidate_width), size);
                        if candidate.lines.len() != 2 {
                            break;
                        }
                        let difference = (width(font, &candidate.lines[0], size)
                            - width(font, &candidate.lines[1], size))
                        .abs();
                        if difference < best_difference && candidate.height <= max_height {
                            best_difference = difference;
                            fitted = candidate;
                        }
                        candidate_width -= 10.0;
                    }
                }
            }
            return fitted;
        }
        if size <= min_size {
            let mut lines = fitted.lines;
            while lines.len() > max_lines || layout(font, lines.clone(), size).height > max_height {
                lines.pop();
            }
            if let Some(last) = lines.last_mut() {
                while width(font, &format!("{last}…"), size) > max_width {
                    let Some((index, _)) = last.grapheme_indices(true).next_back() else {
                        break;
                    };
                    last.truncate(index);
                }
                last.push('…');
            }
            return layout(font, lines, size);
        }
        size = (size - 2.0).max(min_size);
    }
}

pub(crate) fn draw_fitted(
    card: &mut RgbaImage,
    font: &Font,
    fitted: &FittedText,
    position: (f32, f32),
    color: Rgba<u8>,
) {
    for (index, line) in fitted.lines.iter().enumerate() {
        draw_line(
            card,
            font,
            line,
            fitted.size,
            (position.0, position.1 + index as f32 * fitted.size * 1.04),
            color,
        );
    }
}

pub(crate) fn draw_line(
    card: &mut RgbaImage,
    font: &Font,
    text: &str,
    size: f32,
    position: (f32, f32),
    color: Rgba<u8>,
) {
    let size = size * SCALE as f32;
    let bounds = bounds(font, text, size);
    let mut cursor = position.0 * SCALE as f32 - bounds.left;
    let baseline = position.1 * SCALE as f32 + bounds.above;
    let mut previous = None;
    for ch in text.chars() {
        if let Some(previous) = previous {
            cursor += font.horizontal_kern(previous, ch, size).unwrap_or(0.0);
        }
        let (metrics, coverage) = font.rasterize(ch, size);
        let left = cursor.round() as i32 + metrics.xmin;
        let top = baseline.round() as i32 - metrics.ymin - metrics.height as i32;
        for y in 0..metrics.height {
            for x in 0..metrics.width {
                let alpha =
                    (u16::from(coverage[y * metrics.width + x]) * u16::from(color[3]) / 255) as u8;
                if alpha != 0 {
                    blend_pixel(
                        card,
                        left + x as i32,
                        top + y as i32,
                        Rgba([color[0], color[1], color[2], alpha]),
                    );
                }
            }
        }
        cursor += metrics.advance_width;
        previous = Some(ch);
    }
}

pub(crate) fn blend_pixel(card: &mut RgbaImage, x: i32, y: i32, color: Rgba<u8>) {
    if x < 0 || y < 0 || x >= card.width() as i32 || y >= card.height() as i32 {
        return;
    }
    let pixel = card.get_pixel_mut(x as u32, y as u32);
    let alpha = u32::from(color[3]);
    for channel in 0..3 {
        pixel[channel] =
            ((u32::from(color[channel]) * alpha + u32::from(pixel[channel]) * (255 - alpha) + 127)
                / 255) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CardLocale, fonts};

    #[test]
    fn localized_long_titles_fit_the_reserved_area_and_have_glyphs() {
        for (locale, title) in [
            (CardLocale::En, "Courtly Lover’s Wristlet of Aiming"),
            (
                CardLocale::De,
                "Verbessertes Zeremonielles Handgelenkband des Waldläufers",
            ),
            (
                CardLocale::Fr,
                "Bracelet de pisteur des amoureux de la cour",
            ),
            (CardLocale::Ja, "コートリーラヴァー・レンジャーブレスレット"),
            (CardLocale::Ko, "궁정 연인의 유격대 팔찌"),
            (CardLocale::Cn, "宫廷恋人精准手镯与市场交易板价格比较"),
            (CardLocale::Tc, "宮廷戀人精準手鐲與市場交易板價格比較"),
        ] {
            let font = &fonts::for_locale(locale).unwrap().bold;
            for ch in title.chars().filter(|ch| !ch.is_whitespace()) {
                assert_ne!(font.lookup_glyph_index(ch), 0, "missing {ch} in {locale:?}");
            }
            let text = fit(font, title, 96.0, 38.0, 730.0, 302.0, 4);
            assert!(text.width <= 730.0 && text.height <= 302.0, "{locale:?}");
            assert!(
                !text.lines.last().unwrap().ends_with('…'),
                "unexpected truncation: {locale:?}"
            );
        }
    }

    #[test]
    fn all_localized_item_and_job_names_have_glyphs() {
        let mut missing = std::collections::BTreeSet::new();
        for (language, data) in xiv_gen_db::all_locales() {
            let locale = CardLocale::from_code(language.to_path_part()).unwrap();
            let fonts = fonts::for_locale(locale).unwrap();
            for name in data
                .items
                .values()
                .map(|item| &item.name)
                .chain(data.class_jobs.values().map(|job| &job.name))
            {
                for ch in normalized(name)
                    .chars()
                    .filter(|ch| !ch.is_whitespace() && !ch.is_control())
                {
                    if fonts.bold.lookup_glyph_index(ch) == 0
                        || fonts.regular.lookup_glyph_index(ch) == 0
                    {
                        missing.insert(format!("{locale:?}: {ch} (U+{:04X})", ch as u32));
                    }
                }
            }
        }
        assert!(
            missing.is_empty(),
            "missing localized game-name glyphs: {missing:?}"
        );
    }

    #[test]
    fn english_cards_support_other_scripts_in_world_names() {
        let fonts = fonts::for_locale(CardLocale::En).unwrap();
        for ch in "中国 한국 宮廷 神龍 ラムウ"
            .chars()
            .filter(|ch| !ch.is_whitespace())
        {
            assert_ne!(
                fonts.regular.lookup_glyph_index(ch),
                0,
                "missing world glyph {ch}"
            );
        }
    }

    #[test]
    fn localized_social_and_tool_copy_has_glyphs() {
        let tool_keys = [
            "flip_finder",
            "flip_finder_desc",
            "vendor_resale",
            "vendor_resale_desc",
            "recipe_analyzer",
            "recipe_analyzer_desc",
            "fc_crafting_analyzer_title",
            "fc_crafting_desc",
            "leve_analyzer",
            "leve_analyzer_desc",
            "scrip_sources",
            "scrip_sources_desc",
            "venture_analyzer",
            "venture_analyzer_desc",
            "market_trends",
            "market_trends_desc",
            "item_explorer",
            "item_explorer_desc",
            "about",
            "discord_bot",
            "discord_bot_desc",
            "changelog_page_heading",
            "privacy_policy_title",
            "cookie_policy_title",
            "currency_exchange",
            "help_meta_title",
        ];
        let mut missing = std::collections::BTreeSet::new();
        for code in ["en", "ja", "de", "fr", "ko", "cn", "tc"] {
            let catalog_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../ultros-frontend/ultros-app/locales/{code}.json"));
            let catalog: serde_json::Map<String, serde_json::Value> =
                serde_json::from_str(&std::fs::read_to_string(catalog_path).unwrap()).unwrap();
            let fonts = fonts::for_locale(CardLocale::from_code(code).unwrap()).unwrap();
            for key in &tool_keys {
                assert!(
                    catalog.contains_key(*key),
                    "missing {code} catalog key {key}"
                );
            }
            for (key, value) in catalog.iter().filter(|(key, _)| {
                key.starts_with("social_card_") || tool_keys.contains(&key.as_str())
            }) {
                for ch in normalized(value.as_str().expect("plain social card copy"))
                    .chars()
                    .filter(|ch| !ch.is_whitespace() && !ch.is_control())
                {
                    if fonts.bold.lookup_glyph_index(ch) == 0
                        || fonts.regular.lookup_glyph_index(ch) == 0
                    {
                        missing.insert(format!("{code}:{key}: {ch} (U+{:04X})", ch as u32));
                    }
                }
            }
        }
        assert!(
            missing.is_empty(),
            "missing social-copy glyphs: {missing:?}"
        );
    }

    #[test]
    fn invisible_direction_marks_do_not_draw_missing_glyphs() {
        let fonts = fonts::for_locale(CardLocale::Ko).unwrap();
        let clean = fit(&fonts.bold, "궁정 연인", 96.0, 38.0, 730.0, 302.0, 4);
        let marked = fit(
            &fonts.bold,
            "\u{200f}궁정 연인",
            96.0,
            38.0,
            730.0,
            302.0,
            4,
        );
        assert_eq!(clean.lines, marked.lines);
        assert_eq!(clean.width, marked.width);
    }

    #[test]
    fn short_two_line_headlines_do_not_leave_an_orphan_word() {
        let fonts = fonts::for_locale(CardLocale::En).unwrap();
        let title = fit(
            &fonts.bold,
            "Samurai gear sets",
            96.0,
            38.0,
            730.0,
            302.0,
            4,
        );
        assert_eq!(title.lines, ["Samurai", "gear sets"]);
    }

    #[test]
    fn decomposed_accents_use_the_same_layout_as_composed_text() {
        let fonts = fonts::for_locale(CardLocale::Fr).unwrap();
        let composed = fit(
            &fonts.bold,
            "Équipement amélioré",
            96.0,
            38.0,
            730.0,
            302.0,
            4,
        );
        let decomposed = fit(
            &fonts.bold,
            "E\u{301}quipement ame\u{301}liore\u{301}",
            96.0,
            38.0,
            730.0,
            302.0,
            4,
        );
        assert_eq!(composed.lines, decomposed.lines);
        assert_eq!(composed.width, decomposed.width);
    }

    #[test]
    fn cjk_breaks_do_not_start_lines_with_closing_punctuation() {
        let font = &fonts::for_locale(CardLocale::Ja).unwrap().bold;
        let text = wrapped(
            font,
            "「アイテム」の価格を比較。ワールドを選択！",
            50.0,
            200.0,
        );
        assert!(text.len() > 1);
        assert!(
            text.iter()
                .all(|line| !line.starts_with(['」', '。', '！']))
        );
    }

    #[test]
    fn korean_home_title_preserves_words_with_an_oversized_word_fallback() {
        let font = &fonts::for_locale(CardLocale::Ko).unwrap().bold;
        let title = fit(
            font,
            "장터에서 한발 앞서가세요.",
            96.0,
            38.0,
            730.0,
            302.0,
            4,
        );
        assert_eq!(title.lines, ["장터에서 한발", "앞서가세요."]);
        assert!(title.width <= 730.0 && title.height <= 302.0);

        let oversized_word = "아주긴아이템이름".repeat(3);
        let lines = wrapped(font, &oversized_word, 50.0, 200.0);
        assert!(lines.len() > 1);
        assert_eq!(lines.concat(), oversized_word);
        assert!(lines.iter().all(|line| width(font, line, 50.0) <= 200.0));
    }

    #[test]
    fn unbroken_and_oversized_labels_are_bounded_and_ellipsized() {
        let font = &fonts::for_locale(CardLocale::En).unwrap().bold;
        let title = "Supercalifragilisticexpialidocious".repeat(40);
        let text = fit(font, &title, 96.0, 38.0, 730.0, 302.0, 4);
        assert!(text.width <= 730.0 && text.height <= 302.0);
        assert!(text.lines.last().unwrap().ends_with('…'));
    }
}
