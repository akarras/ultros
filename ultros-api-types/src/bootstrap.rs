use serde::{Deserialize, Serialize};

use crate::user::UserData;
use crate::world::WorldData;

/// Server-rendered payload embedded in the initial HTML so the client can
/// hydrate without making `/world_data`, `/detectregion`, and `/current_user`
/// requests on every cold load.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Bootstrap {
    pub world_data: WorldData,
    pub region: String,
    pub current_user: Option<UserData>,
}
