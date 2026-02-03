use flate2::{Compression, FlushCompress};

fn main() {
    let vec: Vec<u8> = vec![];
    let mut flate = flate2::Compress::new(Compression::best(), true);
    let mut output = Vec::with_capacity(vec.len());
    flate
        .compress_vec(vec.as_slice(), &mut output, FlushCompress::Full)
        .unwrap();
    println!("Empty With Full: len={}", output.len());

    let mut flate2 = flate2::Compress::new(Compression::best(), true);
    let mut output2 = Vec::with_capacity(vec.len());
    flate2
        .compress_vec(vec.as_slice(), &mut output2, FlushCompress::Finish)
        .unwrap();
    println!("Empty With Finish: len={}", output2.len());
}
