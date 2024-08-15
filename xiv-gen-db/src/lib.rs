use flate2::FlushDecompress;
use once_cell::sync::OnceCell;

pub static XIV_DATA: OnceCell<xiv_gen::Data> = OnceCell::new();

#[cfg(feature = "embed")]
pub fn bincode() -> &'static [u8] {
    include_bytes!(concat!(env!("OUT_DIR"), "/database.bincode"))
}

#[cfg(feature = "embed")]
pub fn data() -> &'static xiv_gen::Data {
    match XIV_DATA.get() {
        Some(d) => d,
        None => {
            XIV_DATA.set(decompress_data(bincode()).unwrap()).unwrap();
            XIV_DATA.get().unwrap()
        }
    }
}

#[cfg(not(feature = "embed"))]
pub fn data() -> &'static xiv_gen::Data {
    XIV_DATA.get().expect("XIV data not initialized")
}

pub fn try_init(bytes: &[u8]) -> anyhow::Result<()> {
    XIV_DATA.set(decompress_data(bytes)?).unwrap();
    Ok(())
}

pub fn decompress_data(bytes: &[u8]) -> anyhow::Result<xiv_gen::Data> {
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
    use crate::{data, XIV_DATA};

    #[test]
    fn test_embed() {
        data()
            .items
            .iter()
            .find(|(_, i)| i.name == "Grade 2 Gemdraught of Mind")
            .unwrap();
    }
}
