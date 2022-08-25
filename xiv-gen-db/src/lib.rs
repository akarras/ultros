use flate2::FlushDecompress;
use lazy_static::lazy_static;

pub fn decompress_data() -> &'static xiv_gen::Data {
    fn decompress_impl() -> xiv_gen::Data {
        let mut decompressor = flate2::Decompress::new(true);
        let mut data = Vec::new();
        let bytes = include_bytes!(concat!(env!("OUT_DIR"), "/database.bincode"));
        data.reserve(bytes.len() * 5);
        decompressor
            .decompress_vec(bytes, &mut data, FlushDecompress::Sync)
            .unwrap();
        let (data, _) = bincode::decode_from_slice(data.as_slice(), bincode::config::standard())
            .expect("Bin code failed to deserialize, is the database out of date for some reason?");
        data
    }
    lazy_static! {
        pub static ref XIV_DATA: xiv_gen::Data = decompress_impl();
    }
    &XIV_DATA
}
