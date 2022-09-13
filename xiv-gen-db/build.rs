extern crate core;

use flate2::{Compression, FlushCompress};
use std::env;
use std::path::Path;
use xiv_gen::csv_to_bincode::read_data;

fn main() {
    let data = read_data();
    let vec = bincode::encode_to_vec(data, bincode::config::standard()).unwrap();
    let mut flate = flate2::Compress::new(Compression::best(), true);
    let mut output = Vec::new();
    output.reserve(vec.len());
    flate
        .compress_vec(vec.as_slice(), &mut output, FlushCompress::Full)
        .unwrap();
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("database.bincode");
    std::fs::write(dest_path, output.as_slice()).unwrap();
    let start_size = vec.len() as f64 / 1024.0 / 1024.0;
    let compressed_size = output.len() as f64 / 1024.0 / 1024.0;
    let saved_delta = (1.0 - compressed_size / start_size) * 100.0;
    println!(
        "normal {start_size:.2}MB compressed: {compressed_size:.2}MB. saved {saved_delta:.2}%"
    );
    println!("cargo:rerun-if-changed=build.rs");
}
