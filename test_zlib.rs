use std::io::Write;
use flate2::{Compression, FlushCompress, Compress};

fn main() {
    let vec: Vec<u8> = Vec::new(); // Empty input
    let mut flate = Compress::new(Compression::best(), true);
    let mut output = Vec::with_capacity(vec.len());
    flate
        .compress_vec(vec.as_slice(), &mut output, FlushCompress::Full)
        .unwrap();
    println!("Output len: {}", output.len());
}
