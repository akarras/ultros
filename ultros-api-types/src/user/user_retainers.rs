use serde::{Deserialize, Serialize};

use crate::{retainer::Retainer, ActiveListing, FfxivCharacter};

#[derive(Serialize, Deserialize, Default, Debug, PartialEq, PartialOrd, Eq, Ord, Clone)]
pub struct OwnedRetainer {
    pub id: i32,
    pub retainer_id: i32,
    pub discord_id: i64,
    pub character_id: Option<i32>,
    pub weight: Option<i32>,
}

pub type UserRetainerList = Vec<(Option<FfxivCharacter>, Vec<(OwnedRetainer, Retainer)>)>;
pub type UserRetainerListWithListings =
    Vec<(Option<FfxivCharacter>, Vec<(Retainer, Vec<ActiveListing>)>)>;

/// List of all user retainers. User retainer are grouped by character
#[derive(Serialize, Deserialize, Default, Debug, PartialEq, PartialOrd, Eq, Ord, Clone)]
pub struct UserRetainers {
    /// List of all the user's retainers. If no character is associated, it will be placed into the None.
    pub retainers: UserRetainerList,
}

#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone)]
pub struct UserRetainerListings {
    /// List of all the user's retainers. If no character is associated, it will be placed into the None.
    pub retainers: UserRetainerListWithListings,
}
