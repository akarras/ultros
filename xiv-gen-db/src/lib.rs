use anyhow::anyhow;
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
        Language::En => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../data/xiv-db/en.rkyv"
        )),
        Language::Ja => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../data/xiv-db/ja.rkyv"
        )),
        Language::De => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../data/xiv-db/de.rkyv"
        )),
        Language::Fr => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../data/xiv-db/fr.rkyv"
        )),
        Language::Cn => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../data/xiv-db/cn.rkyv"
        )),
        Language::Ko => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../data/xiv-db/ko.rkyv"
        )),
        Language::Tc => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../data/xiv-db/tc.rkyv"
        )),
    }
}

#[cfg(feature = "embed")]
pub fn startup_bytes(lang: Language) -> &'static [u8] {
    match lang {
        Language::En => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../data/xiv-startup/en.rkyv"
        )),
        Language::Ja => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../data/xiv-startup/ja.rkyv"
        )),
        Language::De => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../data/xiv-startup/de.rkyv"
        )),
        Language::Fr => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../data/xiv-startup/fr.rkyv"
        )),
        Language::Cn => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../data/xiv-startup/cn.rkyv"
        )),
        Language::Ko => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../data/xiv-startup/ko.rkyv"
        )),
        Language::Tc => include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../data/xiv-startup/tc.rkyv"
        )),
    }
}

#[cfg(feature = "embed")]
pub fn data() -> &'static xiv_gen::Data {
    if let Some(d) = *XIV_DATA.read().unwrap() {
        return d;
    }
    try_init(embedded_bytes(Language::En)).expect("failed to initialize embedded xiv data");
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

/// Content hash of the `lang` pack this binary was built against: the
/// `<version>` segment of the pack URL and the browser's cache key. Computed
/// by build.rs from the pack bytes, so it changes exactly when the pack does —
/// a deploy that did not touch game data neither re-downloads the pack nor
/// evicts it from the edge cache. Unknown languages resolve to English, which
/// is also what the server falls back to.
pub fn startup_version(lang: &str) -> &'static str {
    match lang {
        "ja" => env!("XIV_STARTUP_VERSION_JA"),
        "de" => env!("XIV_STARTUP_VERSION_DE"),
        "fr" => env!("XIV_STARTUP_VERSION_FR"),
        "cn" => env!("XIV_STARTUP_VERSION_CN"),
        "ko" => env!("XIV_STARTUP_VERSION_KO"),
        "tc" => env!("XIV_STARTUP_VERSION_TC"),
        _ => env!("XIV_STARTUP_VERSION_EN"),
    }
}

pub fn startup_url(lang: &str) -> String {
    format!("/static/startup/{}/{lang}.rkyv", startup_version(lang))
}

pub fn pack_version(lang: &str) -> &'static str {
    match lang {
        "ja" => env!("XIV_PACK_VERSION_JA"),
        "de" => env!("XIV_PACK_VERSION_DE"),
        "fr" => env!("XIV_PACK_VERSION_FR"),
        "cn" => env!("XIV_PACK_VERSION_CN"),
        "ko" => env!("XIV_PACK_VERSION_KO"),
        "tc" => env!("XIV_PACK_VERSION_TC"),
        _ => env!("XIV_PACK_VERSION_EN"),
    }
}

/// The URL the server serves the `lang` pack at (`get_xiv_data_bytes` in the
/// ultros crate). Content-addressed via [`pack_version`], so it is safe to
/// cache immutably.
pub fn pack_url(lang: &str) -> String {
    format!("/static/data/{}/{lang}.rkyv", pack_version(lang))
}

/// Legacy full-pack cache identity. New clients use [`startup_url`] and the
/// HTTP cache instead of IndexedDB.
pub fn pack_cache_key(lang: &str) -> String {
    format!("{}-{lang}", pack_version(lang))
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
    // The pack is a brotli stream (quality 11, 16 MiB window) written by
    // game-data-pack; the English archive inflates to ~17 MB. Decode through
    // the `Read` adapter so the output grows as needed — a fixed output buffer
    // would truncate silently and rkyv would then report a corrupt archive.
    use std::io::Read;
    let mut decoded: Vec<u8> = Vec::new();
    brotli_decompressor::Decompressor::new(bytes, 64 * 1024)
        .read_to_end(&mut decoded)
        .map_err(|e| anyhow!("failed to decompress xiv-gen data: {e}"))?;
    // rkyv requires the byte buffer to be aligned to `FixedIsize` (4 bytes
    // under `size_32`). `Vec<u8>` only guarantees byte alignment, so copy into
    // an `AlignedVec` before deserializing. Without this, `from_bytes` fails
    // with an "unaligned pointer" context error on allocators that don't
    // happen to hand back sufficiently aligned `Vec<u8>` storage (notably
    // Windows).
    let mut aligned = rkyv::AlignedVec::with_capacity(decoded.len());
    aligned.extend_from_slice(&decoded);
    // rkyv's deserialization errors don't implement `std::error::Error` in 0.7,
    // so funnel them through anyhow's string-based fallback.
    let data = rkyv::from_bytes::<xiv_gen::Data>(&aligned)
        .map_err(|e| anyhow!("failed to deserialize xiv-gen data: {e}"))?;
    Ok(data)
}

#[cfg(test)]
mod version_test {
    use super::{pack_cache_key, pack_url, pack_version};

    /// Every language resolves to a 64-bit content hash, distinct packs get
    /// distinct versions, and an unknown language falls back to English's.
    #[test]
    fn pack_versions_are_content_hashes() {
        let langs = ["en", "ja", "de", "fr", "cn", "ko", "tc"];
        for lang in langs {
            let version = pack_version(lang);
            assert_eq!(version.len(), 16, "{lang}: {version}");
            assert!(
                version.chars().all(|c| c.is_ascii_hexdigit()),
                "{lang}: {version}"
            );
        }
        assert_ne!(pack_version("en"), pack_version("ja"));
        assert_eq!(pack_version("klingon"), pack_version("en"));
    }

    /// The URL and the browser cache key both carry the version, so a new pack
    /// is a new URL (edge cache) and a new key (IndexedDB) at the same time.
    #[test]
    fn pack_url_and_cache_key_carry_the_version() {
        let version = pack_version("fr");
        assert_eq!(pack_url("fr"), format!("/static/data/{version}/fr.rkyv"));
        assert_eq!(pack_cache_key("fr"), format!("{version}-fr"));
    }
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
