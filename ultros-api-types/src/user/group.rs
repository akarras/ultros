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

/// A shareable code that grants membership of a group. Mirrors `ListInvite`
/// without a permission — group membership is binary.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct GroupInvite {
    pub id: String,
    pub group_id: i32,
    /// `None` means the invite never expires by use count.
    pub max_uses: Option<i32>,
    pub uses: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct CreateGroupInvite {
    /// Omitted or null means the invite has no use cap. Defaulted so a client
    /// that sends `{}` gets an unlimited invite rather than a 422.
    #[serde(default)]
    pub max_uses: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::{CreateGroupInvite, GroupInvite, GroupSource};

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

    #[test]
    fn create_group_invite_treats_a_missing_max_uses_as_unlimited() {
        // An omitted field and an explicit null have to mean the same thing —
        // "no cap" — or a client that skips the key gets a 422 instead of an
        // invite.
        for body in ["{}", r#"{"max_uses":null}"#] {
            let parsed: CreateGroupInvite = serde_json::from_str(body).unwrap();
            assert_eq!(parsed.max_uses, None, "parsing {body}");
        }
        let parsed: CreateGroupInvite = serde_json::from_str(r#"{"max_uses":5}"#).unwrap();
        assert_eq!(parsed.max_uses, Some(5));
    }

    #[test]
    fn group_invite_round_trips_over_the_wire() {
        for invite in [
            GroupInvite {
                id: "abc123".to_string(),
                group_id: 7,
                max_uses: None,
                uses: 0,
            },
            GroupInvite {
                id: "def456".to_string(),
                group_id: 7,
                max_uses: Some(3),
                uses: 2,
            },
        ] {
            let json = serde_json::to_string(&invite).unwrap();
            assert_eq!(serde_json::from_str::<GroupInvite>(&json).unwrap(), invite);
        }
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
