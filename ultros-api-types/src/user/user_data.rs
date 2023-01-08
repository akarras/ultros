use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default, Debug, PartialEq, PartialOrd, Eq, Ord, Clone)]
pub struct UserData {
    pub id: u64,
    pub username: String,
    pub avatar: String,
}
