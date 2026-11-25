use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SearchResult {
    pub score: f32,
    pub title: String,
    pub result_type: String,
    pub url: String,
    pub icon_id: Option<i32>,
    pub category: Option<String>,
}
