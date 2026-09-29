use std::sync::OnceLock;

use anyhow::{Result, anyhow};
use fontdue::{Font, FontSettings, Metrics};

use crate::CardLocale;

pub(crate) struct Fonts {
    pub regular: Face,
    pub bold: Face,
}

pub(crate) struct Face {
    primary: Font,
    fallback: Option<&'static Font>,
    keep_words: bool,
}

impl Face {
    pub fn keeps_words_together(&self) -> bool {
        self.keep_words
    }

    fn font_for(&self, ch: char) -> &Font {
        if self.primary.lookup_glyph_index(ch) == 0 {
            self.fallback.unwrap_or(&self.primary)
        } else {
            &self.primary
        }
    }

    #[cfg(test)]
    pub fn lookup_glyph_index(&self, ch: char) -> u16 {
        self.font_for(ch).lookup_glyph_index(ch)
    }

    pub fn metrics(&self, ch: char, size: f32) -> Metrics {
        self.font_for(ch).metrics(ch, size)
    }

    pub fn rasterize(&self, ch: char, size: f32) -> (Metrics, Vec<u8>) {
        self.font_for(ch).rasterize(ch, size)
    }

    pub fn horizontal_kern(&self, left: char, right: char, size: f32) -> Option<f32> {
        let left_font = self.font_for(left);
        let right_font = self.font_for(right);
        if std::ptr::eq(left_font, right_font) {
            left_font.horizontal_kern(left, right, size)
        } else {
            None
        }
    }
}

pub(crate) fn for_locale(locale: CardLocale) -> Result<&'static Fonts> {
    type Cache = OnceLock<Result<Fonts, String>>;
    static LATIN: Cache = OnceLock::new();
    static JP: Cache = OnceLock::new();
    static KR: Cache = OnceLock::new();
    static SC: Cache = OnceLock::new();
    static TC: Cache = OnceLock::new();
    macro_rules! face {
        ($name:literal) => {
            include_bytes!(concat!("../assets/fonts/", $name)) as &[u8]
        };
    }
    let (cache, regular, bold) = match locale {
        CardLocale::En | CardLocale::De | CardLocale::Fr => (
            &LATIN,
            face!("Outfit-Regular.ttf"),
            face!("Outfit-Black.ttf"),
        ),
        CardLocale::Ja => (
            &JP,
            face!("NotoSansJP-Regular.otf"),
            face!("NotoSansJP-Bold.otf"),
        ),
        CardLocale::Ko => (
            &KR,
            face!("NotoSansKR-Regular.otf"),
            face!("NotoSansKR-Bold.otf"),
        ),
        CardLocale::Cn => (
            &SC,
            face!("NotoSansSC-Regular.otf"),
            face!("NotoSansSC-Bold.otf"),
        ),
        CardLocale::Tc => (
            &TC,
            face!("NotoSansTC-Regular.otf"),
            face!("NotoSansTC-Bold.otf"),
        ),
    };
    cache
        .get_or_init(|| {
            let settings = FontSettings {
                scale: 192.0,
                ..FontSettings::default()
            };
            let fallback = if locale == CardLocale::Ja {
                None
            } else {
                Some(for_locale(CardLocale::Ja).map_err(|error| error.to_string())?)
            };
            Ok(Fonts {
                regular: Face {
                    primary: Font::from_bytes(regular, settings).map_err(str::to_owned)?,
                    fallback: fallback.map(|fonts| &fonts.regular.primary),
                    keep_words: locale == CardLocale::Ko,
                },
                bold: Face {
                    primary: Font::from_bytes(bold, settings).map_err(str::to_owned)?,
                    fallback: fallback.map(|fonts| &fonts.bold.primary),
                    keep_words: locale == CardLocale::Ko,
                },
            })
        })
        .as_ref()
        .map_err(|error| anyhow!("failed to load embedded social-card font: {error}"))
}
