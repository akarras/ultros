extern crate core;

use flate2::{Compression, FlushCompress};
use std::env;
use std::path::Path;
use xiv_gen::Language;
use xiv_gen::csv_to_rkyv::read_data;

fn main() {
    let languages = [
        Language::En,
        Language::Ja,
        Language::De,
        Language::Fr,
        Language::Cn,
        Language::Ko,
        Language::Tc,
    ];

    for lang in languages {
        let data = read_data(lang);
        // The dataset is large, so use a generous scratch size up front. rkyv's
        // `AllocSerializer` falls back to heap-allocated scratch when needed, but
        // a larger inline buffer avoids extra allocations during build-time
        // serialization.
        let vec = rkyv::to_bytes::<_, 1_048_576>(&data)
            .expect("failed to serialize xiv-gen data with rkyv");
        let mut flate = flate2::Compress::new(Compression::best(), true);
        let mut output = Vec::with_capacity(vec.len());
        flate
            .compress_vec(vec.as_slice(), &mut output, FlushCompress::Full)
            .unwrap();
        assert!(!output.is_empty());
        let out_dir = env::var_os("OUT_DIR").unwrap();
        let dest_path = Path::new(&out_dir).join(format!("database_{}.rkyv", lang.to_path_part()));
        std::fs::write(dest_path, output.as_slice()).unwrap();
        let start_size = vec.len() as f64 / 1024.0 / 1024.0;
        let compressed_size = output.len() as f64 / 1024.0 / 1024.0;
        let saved_delta = (1.0 - compressed_size / start_size) * 100.0;
        println!(
            "{:?} normal {start_size:.2}MB compressed: {compressed_size:.2}MB. saved {saved_delta:.2}%",
            lang
        );
    }
    println!("cargo:rerun-if-changed=build.rs");
}
