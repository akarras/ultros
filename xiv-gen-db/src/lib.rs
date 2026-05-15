use anyhow::anyhow;
use flate2::FlushDecompress;
#[cfg(feature = "embed")]
use std::sync::OnceLock;
use std::sync::RwLock;
#[cfg(feature = "embed")]
use xiv_gen::Language;

// Stored as a leaked `&'static` reference so callers of `data()` can keep using
// the result without holding a lock guard. Swapping the locale leaks the old
// box; bounded by the number of locale switches in a session.
static XIV_DATA: RwLock<Option<&'static xiv_gen::Data>> = RwLock::new(None);

#[cfg(feature = "embed")]
pub fn embedded_bytes(lang: Language) -> &'static [u8] {
    match lang {
        Language::En => include_bytes!(concat!(env!("OUT_DIR"), "/database_en.rkyv")),
        Language::Ja => include_bytes!(concat!(env!("OUT_DIR"), "/database_ja.rkyv")),
        Language::De => include_bytes!(concat!(env!("OUT_DIR"), "/database_de.rkyv")),
        Language::Fr => include_bytes!(concat!(env!("OUT_DIR"), "/database_fr.rkyv")),
        Language::Cn => include_bytes!(concat!(env!("OUT_DIR"), "/database_cn.rkyv")),
        Language::Ko => include_bytes!(concat!(env!("OUT_DIR"), "/database_ko.rkyv")),
        Language::Tc => include_bytes!(concat!(env!("OUT_DIR"), "/database_tc.rkyv")),
    }
}

#[cfg(feature = "embed")]
pub fn data() -> &'static xiv_gen::Data {
    if let Some(d) = *XIV_DATA.read().unwrap() {
        return d;
    }
    let _ = try_init(embedded_bytes(Language::En));
    XIV_DATA.read().unwrap().expect("just initialized")
}

#[cfg(not(feature = "embed"))]
pub fn data() -> &'static xiv_gen::Data {
    XIV_DATA.read().unwrap().expect("XIV data not initialized")
}

// Per-locale lazily-decoded data caches. Each slot is populated on first access
// to `data_for(lang)` and then reused. Lives for the process lifetime — the
// decoded structures are intentionally leaked to hand out `&'static` references
// the same way `XIV_DATA` does.
#[cfg(feature = "embed")]
const LOCALE_COUNT: usize = 7;

#[cfg(feature = "embed")]
const ALL_LANGUAGES: [Language; LOCALE_COUNT] = [
    Language::En,
    Language::Ja,
    Language::De,
    Language::Fr,
    Language::Cn,
    Language::Ko,
    Language::Tc,
];

#[cfg(feature = "embed")]
fn language_index(lang: Language) -> usize {
    match lang {
        Language::En => 0,
        Language::Ja => 1,
        Language::De => 2,
        Language::Fr => 3,
        Language::Cn => 4,
        Language::Ko => 5,
        Language::Tc => 6,
    }
}

#[cfg(feature = "embed")]
static PER_LOCALE: [OnceLock<&'static xiv_gen::Data>; LOCALE_COUNT] = [
    OnceLock::new(),
    OnceLock::new(),
    OnceLock::new(),
    OnceLock::new(),
    OnceLock::new(),
    OnceLock::new(),
    OnceLock::new(),
];

/// Return a `&'static` reference to the decoded data for the given language.
/// Decompresses and leaks on first access for that locale; subsequent calls
/// reuse the cached reference. Unlike `data()`, this does not interact with the
/// mutable `XIV_DATA` global, so it is safe to use alongside locale switches.
#[cfg(feature = "embed")]
pub fn data_for(lang: Language) -> &'static xiv_gen::Data {
    PER_LOCALE[language_index(lang)].get_or_init(|| {
        let decoded =
            decompress_data(embedded_bytes(lang)).expect("embedded xiv-gen data must decode");
        Box::leak(Box::new(decoded))
    })
}

/// Iterate over every supported language paired with its data, populating any
/// locales that haven't been touched yet. Use sparingly — the first call will
/// decode all 7 locales.
#[cfg(feature = "embed")]
pub fn all_locales() -> impl Iterator<Item = (Language, &'static xiv_gen::Data)> {
    ALL_LANGUAGES.iter().map(|&lang| (lang, data_for(lang)))
}

pub fn try_init(bytes: &[u8]) -> anyhow::Result<()> {
    let data = decompress_data(bytes)?;
    let leaked: &'static xiv_gen::Data = Box::leak(Box::new(data));
    *XIV_DATA.write().unwrap() = Some(leaked);
    Ok(())
}

pub fn decompress_data(bytes: &[u8]) -> anyhow::Result<xiv_gen::Data> {
    if bytes.is_empty() {
        return Ok(xiv_gen::Data::default());
    }
    let mut decompressor = flate2::Decompress::new(true);
    let mut data: Vec<u8> = Vec::with_capacity(bytes.len() * 5);
    decompressor
        .decompress_vec(bytes, &mut data, FlushDecompress::Sync)
        .unwrap();
    // rkyv's deserialization errors don't implement `std::error::Error` in 0.7,
    // so funnel them through anyhow's string-based fallback.
    let data = rkyv::from_bytes::<xiv_gen::Data>(&data)
        .map_err(|e| anyhow!("failed to deserialize xiv-gen data: {e}"))?;
    Ok(data)
}

#[cfg(all(test, feature = "embed"))]
mod test {
    use crate::data;

    #[test]
    fn test_embed() {
        data()
            .items
            .iter()
            .find(|(_, i)| i.name == "Grade 2 Gemdraught of Mind")
            .unwrap();
    }
}
