use serde::{Deserialize, Serialize};

use crate::{ActiveListing, FfxivCharacter, retainer::Retainer};
use chrono::NaiveDateTime;

/// Observed game identities have their own namespace; they are never Lodestone IDs.
#[derive(Serialize, Deserialize, Debug, PartialEq, PartialOrd, Eq, Ord, Clone)]
#[serde(tag = "kind", content = "character", rename_all = "snake_case")]
pub enum RetainerCharacter {
    Verified(FfxivCharacter),
    Unverified {
        id: String,
        name: String,
        world_id: i32,
    },
}

impl RetainerCharacter {
    pub fn key(&self) -> String {
        match self {
            Self::Verified(character) => format!("lodestone:{}", character.id),
            Self::Unverified { id, .. } => format!("game:{id}"),
        }
    }
    pub fn name(&self) -> String {
        match self {
            Self::Verified(character) => {
                format!("{} {}", character.first_name, character.last_name)
            }
            Self::Unverified { name, .. } => name.clone(),
        }
    }
    pub fn world_id(&self) -> i32 {
        match self {
            Self::Verified(character) => character.world_id,
            Self::Unverified { world_id, .. } => *world_id,
        }
    }
    pub fn is_unverified(&self) -> bool {
        matches!(self, Self::Unverified { .. })
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Hash, Clone, Copy)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum RetainerListingId {
    Universalis { id: i64 },
    InGame { revision: i64, slot: u8 },
}

/// An account's listing view, not a row in the public market table.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct RetainerListing {
    pub id: RetainerListingId,
    pub world_id: i32,
    pub retainer_id: i32,
    pub item_id: i32,
    pub hq: bool,
    pub price_per_unit: i32,
    pub quantity: i32,
    pub timestamp: NaiveDateTime,
}

impl From<ActiveListing> for RetainerListing {
    fn from(listing: ActiveListing) -> Self {
        Self {
            id: RetainerListingId::Universalis { id: listing.id },
            world_id: listing.world_id,
            retainer_id: listing.retainer_id,
            item_id: listing.item_id,
            hq: listing.hq,
            price_per_unit: listing.price_per_unit,
            quantity: listing.quantity,
            timestamp: listing.timestamp,
        }
    }
}

#[derive(Serialize, Deserialize, Default, Debug, PartialEq, PartialOrd, Eq, Ord, Clone)]
pub struct OwnedRetainer {
    pub id: i32,
    pub retainer_id: i32,
    pub discord_id: i64,
    pub character_id: Option<i32>,
    pub weight: Option<i32>,
}

#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone)]
pub struct AssignRetainerCharacter {
    pub character_id: Option<i32>,
}

pub type UserRetainerList = Vec<(Option<RetainerCharacter>, Vec<(OwnedRetainer, Retainer)>)>;
pub type UserRetainerListWithListings = Vec<(
    Option<RetainerCharacter>,
    Vec<(Retainer, Vec<RetainerListing>)>,
)>;

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
