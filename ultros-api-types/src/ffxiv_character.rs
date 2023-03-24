use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default, Debug, PartialEq, PartialOrd, Eq, Ord, Clone)]
pub struct UnknownCharacter {
    pub id: i32,
    pub name: String,
}

#[derive(Serialize, Deserialize, Default, Debug, PartialEq, PartialOrd, Eq, Ord, Clone)]
pub struct FfxivCharacter {
    pub id: i32,
    pub first_name: String,
    pub last_name: String,
    pub world_id: i32,
}

#[derive(Serialize, Deserialize, Default, Debug, PartialEq, PartialOrd, Eq, Ord, Clone)]
pub struct FfxivCharacterVerification {
    pub id: i32,
    pub character: FfxivCharacter,
    pub verification_string: String,
}
