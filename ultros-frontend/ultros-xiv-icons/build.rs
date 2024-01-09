#![feature(async_closure)]
use flate2::{write::GzEncoder, Compression};
use futures::{stream, StreamExt};
use image::{imageops::FilterType, io::Reader as ImageReader, ImageOutputFormat};
use std::{
    env,
    ffi::OsStr,
    fs::{read_dir, DirEntry},
    io::{Cursor, Write},
    path::{Path, PathBuf},
    time::Instant,
};
use tar::{Builder, Header};
use tempfile::TempDir;
use ultros_api_types::icon_size::IconSize;

/// Resizes all xiv-icons and bundles them
async fn resize_all_images(out_dir: &Path) {
    let dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let assets = format!("{dir}/universalis-assets/icon2x");
    let path = std::fs::canonicalize(&assets).unwrap_or_else(|error| panic!("{error}\n{assets}"));
    println!("opening {path:?}");
    let mut paths = vec![];
    for file in read_dir(&path).unwrap_or_else(|error| panic!("{error}\n{path:?}")) {
        let entry = file.expect("Unable to read file");
        paths.push(entry);
    }
    let len = paths.len();
    let progress_bar = indicatif::ProgressBar::new(len as u64);
    let bar = &progress_bar;
    stream::iter(paths)
        .for_each_concurrent(Some(50), |path| async move {
            let out_dir = out_dir.to_path_buf();
            let handle = tokio::spawn(async move {
                resize_image(path, &out_dir).await;
                // bar.inc(1);
            });
            handle.await.unwrap();
            bar.inc(1);
        })
        .await;
}

async fn resize_image(entry: DirEntry, out_dir: &PathBuf) -> Option<()> {
    // create three sizes of images
    let file = entry.file_name();
    let file = file.to_str()?;
    let (file_name, _) = file.split_once('.')?;
    let path = entry.path();
    let extension = path.extension().and_then(OsStr::to_str)?;
    if extension != "png" {
        return None;
    }
    let data = tokio::fs::read(entry.path())
        .await
        .unwrap_or_else(|error| panic!("{error:?} {entry:?}"));
    let image = ImageReader::new(Cursor::new(&data))
        .with_guessed_format()
        .unwrap_or_else(|error| panic!("{error:?} {entry:?}"));
    let image = image
        .decode()
        .unwrap_or_else(|error| panic!("{error:?}\n{entry:?}"));
    let image = &image;

    // let out_dir = env::var("OUT_DIR").unwrap();
    // let out_dir = out_dir.as_str();
    let resize = async move |icon_size: IconSize| {
        let size = icon_size.get_px_size();
        let resized = image.resize(size, size, FilterType::CatmullRom);

        let file = vec![];
        let mut cursor = Cursor::new(file);
        resized
            .write_to(&mut cursor, ImageOutputFormat::WebP)
            .unwrap();
        let path = format!("{file_name}_{icon_size}.webp");
        let path = out_dir.join(path);
        tokio::fs::write(path, cursor.into_inner())
            .await
            .unwrap_or_else(|e| panic!("{e}\n{out_dir:?}"));

        // resized.save(format!("{out_dir}/{file_name}{icon_size:?}.webp")).unwrap_or_else(|_| panic!("Error saving file {entry:?}"));
    };
    resize(IconSize::Large).await;
    resize(IconSize::Medium).await;
    resize(IconSize::Small).await;
    Some(())
}

async fn compress(path: &PathBuf) {
    let dir = std::fs::read_dir(path).unwrap();
    let mut entries = vec![];
    for entry in dir {
        entries.push(entry.unwrap());
    }
    let values: Vec<_> = stream::iter(entries)
        .map(|entry| async move {
            let file = tokio::fs::read(entry.path()).await.unwrap();
            let file_name = entry.file_name().to_str().unwrap().to_string();
            (file_name, file)
        })
        .buffered(50)
        .collect()
        .await;
    let archive = vec![];
    let archive = Cursor::new(archive);
    let mut tar = Builder::new(archive);
    for (file, data) in values {
        let mut header = Header::new_gnu();
        header.set_path(&file).unwrap();
        header.set_size(data.len() as u64);
        header.set_cksum();
        tar.append_data(&mut header, file, Cursor::new(data))
            .unwrap();
    }
    tar.finish().unwrap();
    let cursor = tar.into_inner().unwrap();
    // Write tar to a compressed file
    let mut compressed = GzEncoder::new(Vec::new(), Compression::best());
    compressed
        .write_all(cursor.into_inner().as_slice())
        .unwrap();
    let compress = compressed.finish().unwrap();
    let out_dir = std::env::var("OUT_DIR").unwrap();
    std::fs::write(format!("{out_dir}/images.tar.gz"), compress).unwrap();
}

#[tokio::main]
async fn main() {
    println!("cargo:rerun-if-changed=./universalis-assets/icon2x");
    println!("cargo:rerun-if-change=./build.rs");
    let instant = Instant::now();
    let temp_dir = TempDir::new().unwrap();
    resize_all_images(temp_dir.path()).await;
    compress(&temp_dir.path().to_path_buf()).await;
    println!(
        "Finished resizing {}ms",
        Instant::now().duration_since(instant).as_millis()
    );
}
