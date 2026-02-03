use flate2::{Compression, FlushCompress};

fn main() {
    let vec = vec![1, 2, 3, 4, 5]; // Dummy data
    let mut flate = flate2::Compress::new(Compression::best(), true);
    let mut output = Vec::with_capacity(vec.len());
    flate
        .compress_vec(vec.as_slice(), &mut output, FlushCompress::Full)
        .unwrap();
    println!("With Full: len={}, {:?}", output.len(), output);

    let mut flate2 = flate2::Compress::new(Compression::best(), true);
    let mut output2 = Vec::with_capacity(vec.len());
    flate2
        .compress_vec(vec.as_slice(), &mut output2, FlushCompress::Finish)
        .unwrap();
    println!("With Finish: len={}, {:?}", output2.len(), output2);
}
