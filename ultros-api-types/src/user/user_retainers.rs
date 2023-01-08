use serde::{Deserialize, Serialize};

use crate::{FfxivCharacter, Retainer};

#[derive(Deserialize, Serialize)]
pub struct OwnedRetainer {
    pub id: i32,
    pub retainer_id: i32,
    pub discord_id: i64,
    pub character_id: Option<i32>,
    pub weight: Option<i32>,
}

/// List of all user retainers. User retainer are grouped by character
#[derive(Deserialize, Serialize)]
pub struct UserRetainers {
    /// List of all the user's retainers. If no character is associated, it will be placed into the None.
    pub retainers: Vec<(Option<FfxivCharacter>, Vec<(OwnedRetainer, Retainer)>)>,
}
