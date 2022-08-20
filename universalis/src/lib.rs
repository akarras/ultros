extern crate core;

use crate::MarketView::{MultiView, SingleView};
use reqwest::{Client, Method, Request, Url};
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use thiserror::Error;
use log::info;

#[derive(Error, Debug)]
pub enum Error {
    #[error("URL parse error: {0}")]
    UrlParseError(#[from] url::ParseError),
    #[error("HTTP Error: {0}")]
    HttpError(#[from] reqwest::Error),
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),
    #[error("Bad ID, listing returned id {0}")]
    BadId(u32),
    #[error("No items were suggested")]
    NoItems,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MateriaView {
    #[serde(rename = "slotID")]
    pub slot_id: Option<u32>,
    #[serde(rename = "materiaID")]
    pub materia_id: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ListingMultiViewData {
    #[serde(rename = "itemID")]
    pub item_id: u32,
    pub last_upload_time: i64,
    pub listings: Vec<ListingView>,
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
    pub stack_size_histogram: Value,
    #[serde(rename = "stackSizeHistogramNQ")]
    pub stack_size_histogram_nq: Value,
    #[serde(rename = "stackSizeHistogramHQ")]
    pub stack_size_histogram_hq: Value,
    pub world_upload_times: Option<Value>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ListingView {
    pub last_review_time: Option<u64>,
    pub price_per_unit: Option<u32>,
    pub quantity: Option<u32>,
    #[serde(rename = "stainID")]
    pub stain_id: Option<u32>,
    pub world_name: Option<String>,
    pub world_id: Option<u8>,
    pub creator_name: Option<String>,
    pub creator_id: Option<u32>,
    pub hq: bool,
    pub is_crafted: bool,
    pub listing_id: Option<u32>,
    pub materia: Vec<MateriaView>,
    pub on_mannequin: bool,
    pub retainer_city: u32,
    #[serde(rename = "retainerID")]
    pub retainer_id: String,
    pub retainer_name: String,
    #[serde(rename = "sellerID")]
    pub seller_id: String,
    pub total: u32,
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
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CurrentlyShownSingleView {
    #[serde(rename = "itemID")]
    pub item_id: u32,
    pub listings: Vec<ListingView>,
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
pub struct WorldId(pub u32);

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
    SingleView()
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    hq: bool,
    price_per_unit: u64,
    quantity: u32,
    buyer_name: String,
    on_mannequin: bool,
    timestamp: u64,
    world_name: WorldName,
    world_id: WorldId,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HistorySingleView {
    item_id: u32,
    entries: Vec<HistoryEntry>,
    last_upload_time: u64,
    dc_name: Option<String>,
    // TODO finish implementation
}

impl UniversalisClient {
    const UNIVERSALIS_BASE_URL: &'static str = "https://universalis.app/api/v2";

    pub fn new() -> Self {
        let client = Client::new();
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
        if item_ids.len() == 0 {
            return Err(Error::NoItems);
        }
        let id_strs: Vec<_> = item_ids.iter().map(|m| m.to_string()).collect();
        let id_str = id_strs.join(",");
        let request = Request::new(
            Method::GET,
            Url::parse(&format!(
                "{}/{world_or_datacenter}/{id_str}",
                Self::UNIVERSALIS_BASE_URL
            ))?,
        );
        info!("Getting current marketboard data: {}", request.url());
        let data = self.client.execute(request).await?;
        // serde struggles with this untagged enum so I just manually decide for it :)
        Ok(if item_ids.len() > 1 {
            MultiView(data.json().await?)
        } else {
            SingleView(data.json().await?)
        })
    }

    pub async fn get_item_history(&self, world_or_datacenter: &str, item_ids: &[i32]) {

    }
}

#[cfg(test)]
mod test {
    use crate::{CurrentlyShownMultiView, MarketView, UniversalisClient};

    #[tokio::test]
    async fn test_get_worlds() {
        let client = UniversalisClient::new();
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
        assert!(data.items.len() > 0);
    }

    #[tokio::test]
    async fn test_marketboard_parse() {
        let client = UniversalisClient::new();
        let items = client
            .marketboard_current_data("Aether", &[3i32])
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
                assert!(v.items.len() > 0);
            }
        }
    }
}
