use flate2::FlushDecompress;
use std::sync::RwLock;
#[cfg(feature = "embed")]
use xiv_gen::Language;

// Stored as a leaked `&'static` reference so callers of `data()` can keep using
// the result without holding a lock guard. Swapping the locale leaks the old
// box; bounded by the number of locale switches in a session.
static XIV_DATA: RwLock<Option<&'static xiv_gen::Data>> = RwLock::new(None);

#[cfg(feature = "embed")]
pub fn bincode(lang: Language) -> &'static [u8] {
    match lang {
        Language::En => include_bytes!(concat!(env!("OUT_DIR"), "/database_en.bincode")),
        Language::Ja => include_bytes!(concat!(env!("OUT_DIR"), "/database_ja.bincode")),
        Language::De => include_bytes!(concat!(env!("OUT_DIR"), "/database_de.bincode")),
        Language::Fr => include_bytes!(concat!(env!("OUT_DIR"), "/database_fr.bincode")),
        Language::Cn => include_bytes!(concat!(env!("OUT_DIR"), "/database_cn.bincode")),
        Language::Ko => include_bytes!(concat!(env!("OUT_DIR"), "/database_ko.bincode")),
        Language::Tc => include_bytes!(concat!(env!("OUT_DIR"), "/database_tc.bincode")),
    }
}

#[cfg(feature = "embed")]
pub fn data() -> &'static xiv_gen::Data {
    if let Some(d) = *XIV_DATA.read().unwrap() {
        return d;
    }
    let _ = try_init(bincode(Language::En));
    XIV_DATA.read().unwrap().expect("just initialized")
}

#[cfg(not(feature = "embed"))]
pub fn data() -> &'static xiv_gen::Data {
    XIV_DATA.read().unwrap().expect("XIV data not initialized")
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
    let (data, _) = bincode::decode_from_slice(&data, xiv_gen::bincode_config())?;
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
