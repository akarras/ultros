use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserGroup {
    pub id: i32,
    pub name: String,
    pub owner_id: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserGroupMember {
    pub group_id: i32,
    pub user_id: i64,
}
