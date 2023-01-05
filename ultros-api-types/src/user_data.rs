use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserData {
    pub id: u64,
    pub username: String,
    pub avatar: String,
}
