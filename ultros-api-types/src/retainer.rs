use serde::{Deserialize, Serialize};

use crate::ActiveListing;

#[derive(Serialize, Deserialize, Default, Debug, PartialEq, PartialOrd, Eq, Ord, Clone)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Retainer {
    pub id: i32,
    pub world_id: i32,
    pub name: String,
    pub retainer_city_id: i32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RetainerListings {
    pub retainer: Retainer,
    pub listings: Vec<ActiveListing>,
}
