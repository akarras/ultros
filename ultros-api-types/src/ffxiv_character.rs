use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct FfxivCharacter {
    pub id: i32,
    pub first_name: String,
    pub last_name: String,
    pub world_id: i32,
}
