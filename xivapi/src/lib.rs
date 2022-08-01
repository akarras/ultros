pub mod models;

use crate::models::item::Item;
use crate::models::recipe::Recipe;
use clap::PossibleValue;
use futures::channel::mpsc::UnboundedSender;
use futures::{SinkExt, Stream};
use itertools::Itertools;
use log::debug;
use serde::de::{DeserializeOwned, MapAccess, Visitor};
use serde::Serialize;
use serde::{Deserialize, Deserializer};
use serde_aux::serde_introspection::serde_introspect;
use serde_json::Value;
use std::error::Error;
use std::fmt;
use std::fmt::{Display, Formatter};
use std::io::Read;
use std::path::PathBuf;
use std::pin::Pin;
use std::time::Duration;
use thiserror::Error;
use tokio::fs::File;
use tokio::io::AsyncReadExt;

#[derive(Error, Debug)]
pub enum XivApiError {
    #[error("{0}")]
    HttpError(#[from] reqwest::Error),
    #[error("error sending data in the channel: {0}")]
    ChannelError(#[from] futures::channel::mpsc::SendError),
    #[error("Error reading file {0}")]
    IoError(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),
}

fn print_pretty_serde_error(path: &str, str: &str, error: &serde_json::Error) {
    let line = error.line();
    let column = error.column();
    let line = str.lines().skip(line - 1).next().unwrap();
    let mut before_str = &line[..column];
    let mut after_str = &line[column + 1..];
    if let Some(start) = before_str.rfind(",") {
        before_str = &before_str[start..];
    }
    if let Some(end) = after_str.find(",") {
        after_str = &after_str[..end];
    }
    eprintln!();
    eprintln!("error parsing {:?}", path);
    eprintln!(
        "error on line: {before_str}{}{after_str}",
        &line[column..=column]
    );
    let pre_pad: String = (0..=before_str.len()).map(|m| '-').collect();
    let post_pad: String = (0..=after_str.len()).map(|m| '-').collect();
    eprintln!("             : {}^{}", pre_pad, post_pad);
    eprintln!("{}", error);
    eprintln!();
}

#[cfg(test)]
mod test {
    use crate::{
        print_pretty_serde_error, query, GenericColumnQuery, Recipe, RecipePage, RecipeRequest,
        XivApiError, XivDataType,
    };
    use itertools::Itertools;
    use serde_aux::serde_introspection::serde_introspect;
    use serde_json::error::Category;
    use serde_json::Error;
    use std::path::PathBuf;
    use std::time::SystemTime;

    #[tokio::test]
    async fn local_recipe_parse_test() {
        // Validate that all recipes can be parsed correctly
        let dir = std::fs::read_dir("../Recipes")
            .unwrap()
            .flat_map(|m| m.ok().map(|p| p.path()))
            .map(|path| async move {
                let str = tokio::fs::read_to_string(path.clone()).await.unwrap();
                let res = serde_json::from_str::<Recipe>(&str);
                // Do some quick introspection on the error because this is impossible to traceback
                if let Err(error) = &res {
                    print_pretty_serde_error(&path, &str, error);
                }
                Ok(res?)
            });
        let futures: Result<Vec<Recipe>, XivApiError> =
            futures::future::join_all(dir).await.into_iter().collect();
        futures.unwrap();
    }

    #[tokio::test]
    async fn recipe_test() {
        let recipe = query(RecipeRequest(1)).await.unwrap();
        let _ingredients: Vec<_> = recipe
            .ingredients()
            .map(|(q, i)| (q, &i.name_en, i.id))
            .collect();
    }

    #[tokio::test]
    async fn columns_test() {
        let columns = query(XivDataType::Recipe).await.unwrap();
        assert_eq!(columns.0[0], "AmountIngredient0");
    }
}

#[derive(Clone, Debug)]
pub struct GenericColumnQuery<'a> {
    page: i32,
    page_column_query: &'a XivDataType,
    columns: &'a Columns,
}

impl XivDataQuery for GenericColumnQuery<'_> {
    type Data = PaginatedResults<Value>;

    fn get_path(&self) -> String {
        let path = format!(
            "/{}?page={}&columns={}",
            self.page_column_query, self.page, self.columns
        );
        path
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct IndexRecord {
    pub name: String,
    pub id: i64,
}

impl Display for IndexRecord {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Index(pub Vec<IndexRecord>);

impl<'a> Index {
    pub fn search(&'a self, search: &str) -> impl Iterator<Item = &IndexRecord> {
        let search = search.to_owned();
        self.0
            .iter()
            .filter(move |record| record.name.contains(&search))
    }
}

pub fn get_index(xiv_data_type: &XivDataType) -> Index {
    let index = std::fs::File::open(format!("./{}_index.json", xiv_data_type)).unwrap();
    serde_json::from_reader(index).unwrap()
}

#[derive(Clone, Debug)]
pub enum XivDataType {
    Action,
    Recipe,
    Item,
    Quest,
}

impl clap::ValueEnum for XivDataType {
    fn value_variants<'a>() -> &'a [Self] {
        &[Self::Recipe, Self::Action, Self::Quest, Self::Item]
    }

    fn to_possible_value<'a>(&self) -> Option<PossibleValue<'a>> {
        match self {
            XivDataType::Action => Some(PossibleValue::new("action")),
            XivDataType::Recipe => Some(PossibleValue::new("recipe")),
            XivDataType::Item => Some(PossibleValue::new("item")),
            XivDataType::Quest => Some(PossibleValue::new("quest")),
        }
    }
}

pub struct DeepCrawl {
    data: XivDataType,
    start_page: Option<i32>,
    max_page: Option<i32>,
}

async fn deep_crawl_impl(
    data_type: XivDataType,
    start_page: i32,
    stop_page: Option<i32>,
    columns: Columns,
    mut sender: UnboundedSender<Vec<Value>>,
) -> Result<(), XivApiError> {
    let (total_pages, value) = query(GenericColumnQuery {
        page: start_page,
        page_column_query: &data_type,
        columns: &columns,
    })
    .await
    .map(|m| (m.pagination.page_total, m.results))?;
    // send our first result
    sender.send(value).await?;
    let last_page = stop_page
        .map(|m| m.min(total_pages as i32))
        .unwrap_or(total_pages as i32);
    // create a iterator that maps over each of the columns
    let futures: Vec<Vec<_>> = (start_page + 1..=last_page)
        .into_iter()
        .chunks(10)
        .into_iter()
        .map(|chunk| {
            chunk
                .map(|page| {
                    query(GenericColumnQuery {
                        page,
                        page_column_query: &data_type,
                        columns: &columns,
                    })
                })
                .collect()
        })
        .collect();
    for chunk in futures {
        for value in futures::future::join_all(chunk.into_iter()).await {
            let value = value?;
            sender.send(value.results).await?;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Ok(())
}

impl DeepCrawl {
    pub fn new(data: XivDataType, start_page: Option<i32>, max_page: Option<i32>) -> Self {
        Self {
            data,
            start_page,
            max_page,
        }
    }

    pub async fn deep_crawl(&self) -> Result<Pin<Box<dyn Stream<Item = Vec<Value>>>>, XivApiError> {
        let data_type = self.data.clone();
        let start_page = self.start_page.unwrap_or(1);
        let max_page = self.max_page.clone();
        let columns = query(self.data.clone()).await?;
        let (sender, receiver) = futures::channel::mpsc::unbounded();
        tokio::spawn(async move {
            if let Err(e) = deep_crawl_impl(data_type, start_page, max_page, columns, sender).await
            {
                log::error!("{:?}", e);
            }
        });

        Ok(Box::pin(receiver))
    }
}

/// Columns collects all the top level keys from a request
/// Ex. https://xivapi.com/Recipe/1 returns all columns found
#[derive(Debug)]
pub struct Columns(Vec<String>);

impl Display for Columns {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.join(","))
    }
}

impl<'de> Deserialize<'de> for Columns {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ColumnVisitor;

        impl<'de> Visitor<'de> for ColumnVisitor {
            type Value = Columns;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("expected a map")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut columns = vec![];
                while let Some(key) = map.next_key()? {
                    columns.push(key);
                    let _value: Value = map.next_value()?;
                }
                Ok(Columns(columns))
            }
        }

        deserializer.deserialize_map(ColumnVisitor)
    }
}

impl Display for XivDataType {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                XivDataType::Action => "Action",
                XivDataType::Recipe => "Recipe",
                XivDataType::Item => "Item",
                XivDataType::Quest => "Quest",
            }
        )
    }
}

impl XivDataQuery for XivDataType {
    type Data = Columns;

    fn get_path(&self) -> String {
        format!("/{}/1", self)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Pagination {
    pub page: u32,
    pub page_next: Option<u32>,
    pub page_prev: Option<u32>,
    pub page_total: u32,
    pub results: u32,
    pub results_per_page: u32,
    pub results_total: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PaginatedResults<T> {
    pub pagination: Pagination,
    pub results: Vec<T>,
}

pub trait XivDataQuery {
    type Data: DeserializeOwned;

    fn get_path(&self) -> String;
}

pub async fn query<T: XivDataQuery>(query: T) -> Result<T::Data, XivApiError>
where
    for<'de> <T as XivDataQuery>::Data: DeserializeOwned,
{
    debug!("preforming query {:?}", query.get_path());

    let get = reqwest::get(format!("https://xivapi.com{}", query.get_path())).await?;
    Ok(get.json().await?)
}

pub fn disk_query<T: XivDataQuery>(query: T) -> Result<T::Data, XivApiError> {
    let disk_path = format!(".{}.json", query.get_path());
    println!("opening disk path {}", disk_path);
    let mut file = std::fs::File::open(&disk_path)?;
    let mut string = String::new();
    file.read_to_string(&mut string)?;
    let value = serde_json::from_str(&string);
    if let Err(e) = &value {
        print_pretty_serde_error(&disk_path, &string, e);
    }
    Ok(value?)
}

pub async fn disk_query_async<T: XivDataQuery>(query: T) -> Result<T::Data, XivApiError> {
    let disk_path = format!(".{}.json", query.get_path());
    println!("opening disk path {}", disk_path);
    let mut file = File::open(disk_path).await?;
    let mut string = String::new();
    file.read_to_string(&mut string).await?;
    let value = serde_json::from_str(&string)?;
    Ok(value)
}

/// Requests a given recipe with the given ID
pub struct ItemRequest(u32);

impl ItemRequest {
    pub fn new(id: u32) -> Self {
        Self { 0: id }
    }
}

impl XivDataQuery for ItemRequest {
    type Data = Item;

    fn get_path(&self) -> String {
        format!("/Item/{}", self.0)
    }
}

/// Requests a given recipe with the given ID
pub struct RecipeRequest(u32);

impl RecipeRequest {
    pub fn new(id: u32) -> Self {
        Self { 0: id }
    }
}

impl XivDataQuery for RecipeRequest {
    type Data = Recipe;

    fn get_path(&self) -> String {
        format!("/Recipe/{0}", self.0)
    }
}

pub struct RecipePage {
    pub page: u32,
}

impl XivDataQuery for RecipePage {
    type Data = PaginatedResults<Recipe>;

    fn get_path(&self) -> String {
        // let columns = self.columns.join(",");
        let recipe = serde_introspect::<Recipe>();
        let columns = recipe.join(",");
        format!("/Recipe?page={}&columns={}", self.page, columns)
    }
}
