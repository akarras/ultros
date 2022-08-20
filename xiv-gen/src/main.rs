extern crate core;

mod custom_bool_deserialize;

use clap::Parser;
use csv::{Error, ErrorKind};
use flate2::{Compression, FlushCompress};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::path::Path;

fn read_csv<T: DeserializeOwned>(path: &str) -> Vec<T> {
    let mut csv = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_path(path)
        .expect("Failed to open csv");
    let str = std::fs::read_to_string(path).unwrap();
    let headers: Vec<String> = csv
        .records()
        .skip(1)
        .next()
        .unwrap()
        .unwrap()
        .iter()
        .map(|s| s.to_string())
        .collect();
    // line 2
    csv.deserialize()
        .skip(2)
        .map(|m| {
            if let Err(e) = &m {
                if let Some(position) = e.position() {
                    match e.kind() {
                        ErrorKind::Deserialize { pos, err } => {
                            let field = err.field().unwrap();
                            let field_name = &headers[field as usize];
                            eprintln!("{field}: {field_name}");
                        }
                        _ => {}
                    }
                    let byte = position.byte() as usize;
                    // let value = &str[byte-10..=byte+10];
                    eprintln!("{e:?}");
                }
            }
            m.unwrap()
        })
        .collect()
}
include!(concat!(env!("OUT_DIR"), "/serialization.rs"));

#[derive(Debug, Parser)]
#[clap(author, version, about, long_about = None)]
enum Args {
    // #[clap(long)]
    CsvToBinCode,
}

fn main() {
    let data = read_data();
    let vec = bincode::encode_to_vec(data, bincode::config::standard()).unwrap();
    let mut flate = flate2::Compress::new(Compression::best(), true);
    let mut output = Vec::new();
    output.reserve(vec.len());
    flate
        .compress_vec(vec.as_slice(), &mut output, FlushCompress::Full)
        .unwrap();
    std::fs::write("./database.bincode", output.as_slice()).unwrap();
    let start_size = vec.len() as f64 / 1024.0 / 1024.0;
    let compressed_size = output.len() as f64 / 1024.0 / 1024.0;
    let saved_delta = (1.0 - compressed_size / start_size) * 100.0;
    println!(
        "normal {start_size:.2}MB compressed: {compressed_size:.2}MB. saved {saved_delta:.2}%"
    );
}
