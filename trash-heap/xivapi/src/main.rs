use clap::Parser;
use futures::{Stream, StreamExt};
use log::info;
use serde_json::Value;
use std::path::PathBuf;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use xivapi::{DeepCrawl, IndexRecord, XivApiError, XivDataType};

async fn fetch_data(
    data_type: XivDataType,
    save_dir: PathBuf,
    start_page: Option<i32>,
    max_page: Option<i32>,
) -> Result<(), XivApiError> {
    let stream = DeepCrawl::new(data_type.clone(), start_page, max_page)
        .deep_crawl()
        .await?;
    save_recipes(data_type, stream, save_dir).await;
    Ok(())
}

/// saves recipes that are sent over the channel it contains
async fn save_recipes(
    data_type: XivDataType,
    mut recv: impl Stream<Item = Vec<Value>> + Unpin,
    path_buf: PathBuf,
) {
    info!("creating dir");
    let data_str = format!("{:?}", data_type);
    tokio::fs::create_dir_all(path_buf.join(&data_str))
        .await
        .unwrap();
    while let Some(recipes) = recv.next().await {
        // save each recipe as an individual within the path
        futures::future::join_all(recipes.iter().map(|recipe| {
            let recipe_bytes = serde_json::to_vec(&recipe).unwrap();
            tokio::fs::write(
                path_buf.join(format!(
                    "{data_str}/{}.json",
                    recipe.as_object().unwrap().get("ID").unwrap()
                )),
                recipe_bytes,
            )
        }))
        .await;
    }
}

#[derive(Debug, Parser)]
struct Args {
    #[clap(short, long, value_parser, default_value = "false")]
    index_create: bool,
    #[clap(short, long, value_parser, default_value = "./")]
    path: PathBuf,
    #[clap(short, long, value_parser)]
    start_page: Option<i32>,
    #[clap(short, long, value_parser)]
    end_page: Option<i32>,
    #[clap(short, long, value_parser)]
    data_type: XivDataType,
}

#[tokio::main]
async fn main() {
    let args: Args = Args::parse();
    if args.index_create {
        let path = args.path.join(format!("{}/", args.data_type));
        let dir = std::fs::read_dir(path).unwrap();
        let values = dir
            .map(|m| m.unwrap().path())
            .filter(|m| {
                m.extension()
                    .map(|e| {
                        println!("{e:?}");
                        e.eq_ignore_ascii_case("json")
                    })
                    .unwrap_or_default()
            })
            .map(|path| async move {
                println!("path {path:?}");
                let mut file = File::open(path).await.unwrap();
                let mut json_str = String::new();
                file.read_to_string(&mut json_str).await.unwrap();
                let json: Value = serde_json::from_str(&json_str).unwrap();
                let name = json.get("Name")?.as_str()?.to_string();
                let id = json.get("ID")?.as_i64()?;
                let value = IndexRecord { name, id };
                Some(value)
            });
        let values: Vec<_> = futures::future::join_all(values)
            .await
            .into_iter()
            .flatten()
            .collect();
        assert!(values.len() > 0);
        let path = args.path.join(format!("{}_index.json", args.data_type));
        let bytes = serde_json::to_vec(&values).unwrap();
        std::fs::write(path, bytes).unwrap();
        println!("{values:?}");
    } else {
        let path = args.path;
        fetch_data(args.data_type, path, args.start_page, args.end_page)
            .await
            .unwrap();
    }
}
