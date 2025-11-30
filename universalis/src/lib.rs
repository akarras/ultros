#[cfg(feature = "websocket")]
pub mod websocket;
#[cfg(feature = "websocket")]
pub use websocket::WebsocketClient;
extern crate core;

use crate::MarketView::{MultiView, SingleView};
use chrono::{DateTime, Local};
use log::info;
use reqwest::{Client, Method, Request, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::{TimestampMilliSeconds, TimestampSeconds, formats::Flexible, serde_as};
use std::collections::{BTreeMap, HashMap};
use thiserror::Error;

#[derive(Hash, Copy, Clone, Debug, Deserialize, Serialize, Eq, PartialEq, PartialOrd, Ord)]
pub struct ItemId(pub i32);

#[derive(Error, Debug)]
pub enum Error {
    #[error("URL parse error: {0}")]
    UrlParseError(#[from] url::ParseError),
    #[error("HTTP Error: {0}")]
    HttpError(#[from] reqwest::Error),
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),
    #[error("Error deserializing BSON {0}")]
    BsonDeserializeError(#[from] bson::de::Error),
    #[error("Error serializing bson {0}")]
    BsonSerializeError(#[from] bson::ser::Error),
    #[error("Websocket error {0}")]
    TungsteniteError(#[from] Box<async_tungstenite::tungstenite::Error>),
    #[error("Bad ID, listing returned id {0}")]
    BadId(u32),
    #[error("No items were suggested")]
    NoItems,
}

impl From<async_tungstenite::tungstenite::Error> for Error {
    fn from(value: async_tungstenite::tungstenite::Error) -> Self {
        Self::TungsteniteError(Box::new(value))
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MateriaView {
    #[serde(rename = "slotID")]
    pub slot_id: Option<u32>,
    #[serde(rename = "materiaID")]
    pub materia_id: u32,
}

pub type StackSizeHistogram = HashMap<u64, u16>;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ListingMultiViewData {
    #[serde(rename = "itemID")]
    pub item_id: u32,
    pub last_upload_time: i64,
    pub listings: Vec<ListingView>,
    pub recent_history: Vec<SaleView>,
    pub current_average_price: f64,
    #[serde(rename = "currentAveragePriceNQ")]
    pub current_average_price_nq: f64,
    #[serde(rename = "currentAveragePriceHQ")]
    pub current_average_price_hq: f64,
    pub regular_sale_velocity: f64,
    pub nq_sale_velocity: f64,
    pub hq_sale_velocity: f64,
    pub average_price: f64,
    #[serde(rename = "averagePriceNQ")]
    pub average_price_nq: f64,
    #[serde(rename = "averagePriceHQ")]
    pub average_price_hq: f64,
    pub min_price: f64,
    #[serde(rename = "minPriceNQ")]
    pub min_price_nq: f64,
    #[serde(rename = "minPriceHQ")]
    pub min_price_hq: f64,
    pub max_price: f64,
    #[serde(rename = "maxPriceNQ")]
    pub max_price_nq: f64,
    #[serde(rename = "maxPriceHQ")]
    pub max_price_hq: f64,
    pub stack_size_histogram: StackSizeHistogram,
    #[serde(rename = "stackSizeHistogramNQ")]
    pub stack_size_histogram_nq: StackSizeHistogram,
    #[serde(rename = "stackSizeHistogramHQ")]
    pub stack_size_histogram_hq: StackSizeHistogram,
    pub world_upload_times: Option<Value>,
}

#[serde_as]
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ListingView {
    #[serde_as(as = "TimestampSeconds<i64, Flexible>")]
    pub last_review_time: DateTime<Local>,
    pub price_per_unit: Option<u32>,
    pub quantity: Option<u32>,
    #[serde(rename = "stainID")]
    pub stain_id: Option<u32>,
    pub world_name: Option<String>,
    #[serde(rename = "worldID")]
    pub world_id: Option<u16>,
    pub creator_name: Option<String>,
    /// UUID
    #[serde(rename = "creatorID")]
    pub creator_id: Option<String>,
    pub hq: bool,
    pub is_crafted: bool,
    pub listing_id: Option<u32>,
    pub materia: Vec<MateriaView>,
    pub on_mannequin: bool,
    pub retainer_city: u32,
    /// UUID
    #[serde(rename = "retainerID")]
    pub retainer_id: Option<String>,
    pub retainer_name: String,
    #[serde(rename = "sellerID")]
    pub seller_id: Option<String>,
    pub total: u32,
    pub tax: i32,
}

#[serde_as]
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SaleView {
    pub hq: bool,
    pub price_per_unit: i32,
    pub quantity: i32,
    #[serde_as(as = "TimestampSeconds<i64, Flexible>")]
    pub timestamp: DateTime<Local>,
    pub on_mannequin: bool,
    pub world_name: Option<String>,
    #[serde(rename = "worldID")]
    pub world_id: Option<WorldId>,
    pub buyer_name: String,
    pub total: i32,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
pub enum MarketView {
    SingleView(CurrentlyShownSingleView),
    MultiView(CurrentlyShownMultiView),
}

impl MarketView {
    pub fn get_listings_for_item_id(&self, id: u32) -> Result<&Vec<ListingView>, Error> {
        match self {
            SingleView(v) => {
                if v.item_id != id {
                    return Err(Error::BadId(v.item_id));
                }

                Ok(&v.listings)
            }
            MultiView(v) => v
                .items
                .get(&id)
                .ok_or(Error::BadId(id))
                .map(|m| &m.listings),
        }
    }

    pub fn items(self) -> impl Iterator<Item = (ItemId, Vec<ListingView>, Vec<SaleView>)> {
        match self {
            SingleView(single) => vec![(
                ItemId(single.item_id as i32),
                single.listings,
                single.recent_history,
            )]
            .into_iter(),
            MultiView(multi) => multi
                .items
                .into_iter()
                .map(|(i, d)| (ItemId(i as i32), d.listings, d.recent_history))
                .collect::<Vec<_>>()
                .into_iter(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CurrentlyShownSingleView {
    #[serde(rename = "itemID")]
    pub item_id: u32,
    pub listings: Vec<ListingView>,
    pub recent_history: Vec<SaleView>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CurrentlyShownMultiView {
    #[serde(rename = "itemIDs")]
    pub item_ids: Vec<u32>,
    pub items: HashMap<u32, ListingMultiViewData>,
    pub unresolved_items: Vec<Value>,
    pub dc_name: Option<String>,
}

pub struct UniversalisClient {
    client: Client,
}

#[derive(Serialize, Deserialize, Debug, Copy, Clone, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct WorldId(pub i32);

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct RegionName(pub String);

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct DataCenterName(pub String);

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DataCentersView(pub Vec<DataCenterView>);

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DataCenterView {
    pub name: DataCenterName,
    pub region: RegionName,
    pub worlds: Vec<WorldId>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WorldsView(pub Vec<WorldView>);

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct WorldName(pub String);

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WorldView {
    pub id: WorldId,
    pub name: WorldName,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum HistoryView {
    SingleView(Box<HistorySingleView>),
    MultiView(Box<HistoryMultiView>),
}

#[serde_as]
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub hq: bool,
    pub price_per_unit: u64,
    pub quantity: u32,
    pub buyer_name: Option<String>,
    pub on_mannequin: Option<bool>,
    #[serde_as(as = "TimestampSeconds<i64, Flexible>")]
    pub timestamp: DateTime<Local>,
    pub world_name: Option<WorldName>,
    #[serde(rename = "worldID")]
    pub world_id: Option<WorldId>,
}

#[serde_as]
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HistoryMultiView {
    #[serde(rename = "itemIDs")]
    pub item_ids: Vec<u32>,
    pub items: BTreeMap<u32, HistorySingleView>,
    pub dc_name: Option<String>,
    pub unresolved_items: Vec<u32>,
}

#[serde_as]
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HistorySingleView {
    #[serde(rename = "itemID")]
    pub item_id: u32,
    pub entries: Vec<HistoryEntry>,
    #[serde_as(as = "TimestampMilliSeconds<i64, Flexible>")]
    pub last_upload_time: DateTime<Local>,
    pub dc_name: Option<String>,
    pub stack_size_histogram: StackSizeHistogram,
    #[serde(rename = "stackSizeHistogramHQ")]
    pub stack_size_histogram_hq: StackSizeHistogram,
    #[serde(rename = "stackSizeHistogramNQ")]
    pub stack_size_histogram_nq: StackSizeHistogram,
    pub nq_sale_velocity: f64,
    pub hq_sale_velocity: f64,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct MostRecentlyUpdatedItemsView {
    pub items: Vec<WorldItemRecencyView>,
}

#[serde_as]
#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WorldItemRecencyView {
    // The item ID.
    #[serde(rename = "itemID")]
    pub item_id: i32,
    // The last upload time for the item on the listed world.
    #[serde_as(as = "TimestampMilliSeconds<i64, Flexible>")]
    pub last_upload_time: DateTime<Local>,
    // The world ID.
    #[serde(rename = "worldID")]
    pub world_id: i32,
    // The world name.
    pub world_name: Option<String>,
}

pub enum WorldOrDatacenter<'a> {
    World(&'a str),
    Datacenter(&'a str),
}

impl UniversalisClient {
    const UNIVERSALIS_BASE_URL: &'static str = "https://universalis.app/api/v2";

    pub fn new(user_agent: impl ToString) -> Self {
        let client = Client::builder()
            .user_agent(user_agent.to_string())
            .build()
            .unwrap();

        UniversalisClient { client }
    }

    pub async fn get_data_centers(&self) -> Result<DataCentersView, Error> {
        let data_centers = Request::new(
            Method::GET,
            Url::parse(&format!("{}/data-centers", Self::UNIVERSALIS_BASE_URL))?,
        );
        Ok(self.client.execute(data_centers).await?.json().await?)
    }

    pub async fn get_worlds(&self) -> Result<WorldsView, Error> {
        let data_centers = Request::new(
            Method::GET,
            Url::parse(&format!("{}/worlds", Self::UNIVERSALIS_BASE_URL))?,
        );
        Ok(self.client.execute(data_centers).await?.json().await?)
    }

    pub async fn marketboard_current_data(
        &self,
        world_or_datacenter: &str,
        item_ids: &[i32],
    ) -> Result<MarketView, Error> {
        if item_ids.is_empty() {
            return Err(Error::NoItems);
        }
        let id_str = Self::ids_to_string(item_ids);
        let request = Request::new(
            Method::GET,
            Url::parse(&format!(
                "{}/{world_or_datacenter}/{id_str}",
                Self::UNIVERSALIS_BASE_URL
            ))?,
        );
        info!("Getting current marketboard data: {}", request.url());
        let response = self.client.execute(request).await?;
        // serde struggles with this untagged enum so I just manually decide for it :)
        Ok(if item_ids.len() == 1 {
            SingleView(response.json().await?)
        } else {
            MultiView(response.json().await?)
        })
    }

    pub async fn get_item_history(
        &self,
        world_or_datacenter: &str,
        item_ids: &[i32],
    ) -> Result<HistoryView, Error> {
        let id_str = Self::ids_to_string(item_ids);
        let url = format!(
            "{}/history/{world_or_datacenter}/{}?entries=4800",
            Self::UNIVERSALIS_BASE_URL,
            id_str
        );
        info!("getting historical marketboard data: {}", url);
        let response = self.client.get(url).send().await?;
        Ok(if item_ids.len() == 1 {
            HistoryView::SingleView(response.json().await?)
        } else {
            HistoryView::MultiView(response.json().await?)
        })
    }

    pub async fn recently_updated_items(
        &self,
        filter: WorldOrDatacenter<'_>,
        entries: u8,
    ) -> Result<MostRecentlyUpdatedItemsView, Error> {
        let world_filter_str = match filter {
            WorldOrDatacenter::World(world) => format!("world={world}"),
            WorldOrDatacenter::Datacenter(datacenter) => format!("datacenter={datacenter}"),
        };
        let url = format!(
            "{}/extra/stats/most-recently-updated?entries={entries}&{world_filter_str}",
            Self::UNIVERSALIS_BASE_URL
        );
        info!("getting recently updated items {}", url);
        Ok(self.client.get(url).send().await?.json().await?)
    }

    fn ids_to_string(item_ids: &[i32]) -> String {
        let id_strs: Vec<_> = item_ids.iter().map(|m| m.to_string()).collect();
        id_strs.join(",")
    }
}

#[cfg(test)]
mod test {
    use crate::{CurrentlyShownMultiView, MarketView, UniversalisClient};

    #[tokio::test]
    async fn test_get_worlds() {
        let client = UniversalisClient::new("ultros");
        client.get_worlds().await.unwrap();
        client.get_data_centers().await.unwrap();
    }

    #[tokio::test]
    async fn test_marketboard_multiview_parse() {
        let data: CurrentlyShownMultiView =
            reqwest::get("https://universalis.app/api/v2/Aether/15858,30862")
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
        assert!(!data.items.is_empty());
    }

    #[tokio::test]
    async fn test_marketboard() {
        let client = UniversalisClient::new("ultros");
        let items = client
            .marketboard_current_data("Aether", &[24144])
            .await
            .unwrap();
        match items {
            MarketView::SingleView(s) => assert!(s.listings.len() > 1),
            MarketView::MultiView(_) => panic!("unexpected"),
        }

        let multiview = client
            .marketboard_current_data("Aether", &[15858, 30862])
            .await
            .unwrap();
        match multiview {
            MarketView::SingleView(_) => panic!("unexpected"),
            MarketView::MultiView(v) => {
                assert!(!v.items.is_empty());
            }
        }
    }

    #[tokio::test]
    async fn test_history() {
        let client = UniversalisClient::new("ultros");

        // let data : HistorySingleView = serde_json::from_str(&test_data).unwrap();
        client.get_item_history("Aether", &[15858]).await.unwrap();
        client.get_item_history("Aether", &[3, 2]).await.unwrap();
    }

    #[tokio::test]
    async fn test_local_world_history() {
        let client = UniversalisClient::new("ultros");
        client
            .get_item_history("Sargatanas", &[36693])
            .await
            .unwrap();
        client
            .get_item_history("Sargatanas", &[36693, 2])
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_recently_updated() {
        let client = UniversalisClient::new("ultros");
        let entries = client
            .recently_updated_items(crate::WorldOrDatacenter::World("Sargatanas"), 200)
            .await
            .unwrap();
        assert_eq!(entries.items.len(), 200);
    }
}
