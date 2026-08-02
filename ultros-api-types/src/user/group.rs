use serde::{Deserialize, Serialize};

/// How a group's membership is maintained. Stored as a `smallint` on
/// `user_group.source`, mirroring the `ListPermission` idiom.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum GroupSource {
    /// Members are added and removed by the owner. The default.
    Manual = 0,
    /// Created from a Discord guild. Membership is still manual in phase 1;
    /// the guild link supplies the group's identity (name, icon).
    DiscordGuild = 1,
}

impl From<i16> for GroupSource {
    fn from(value: i16) -> Self {
        match value {
            1 => GroupSource::DiscordGuild,
            _ => GroupSource::Manual,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserGroup {
    pub id: i32,
    pub name: String,
    pub owner_id: i64,
    /// Set when the group was created from a Discord guild.
    pub guild_id: Option<i64>,
    /// Guild icon captured at creation time. May be stale; render a fallback
    /// rather than treating this as authoritative.
    pub guild_icon_url: Option<String>,
    pub source: GroupSource,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserGroupMember {
    pub group_id: i32,
    pub user_id: i64,
    pub username: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CreateGroup {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CreateGroupFromGuild {
    pub guild_id: i64,
}

#[cfg(test)]
mod tests {
    use super::GroupSource;

    #[test]
    fn group_source_round_trips_through_its_database_representation() {
        for source in [GroupSource::Manual, GroupSource::DiscordGuild] {
            assert_eq!(GroupSource::from(source as i16), source);
        }
    }

    #[test]
    fn unknown_group_source_values_fall_back_to_manual() {
        // Rows written by a future version must not make the whole group
        // unreadable; degrading to Manual keeps membership owner-managed,
        // which is the safe direction.
        assert_eq!(GroupSource::from(0), GroupSource::Manual);
        assert_eq!(GroupSource::from(99), GroupSource::Manual);
        assert_eq!(GroupSource::from(-1), GroupSource::Manual);
    }
}

/// A Discord guild the authenticated user may turn into a group: the bot is a
/// member of it, and the user has Manage Server or Administrator there.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct DiscordManageableGuild {
    pub id: i64,
    pub name: String,
    pub icon_url: Option<String>,
    /// Set when a group already exists for this guild, so the picker can show
    /// it as taken instead of failing the create.
    pub existing_group_id: Option<i32>,
}
