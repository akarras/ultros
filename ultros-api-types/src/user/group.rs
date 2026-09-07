use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// How a group's membership is maintained. Stored as a `smallint` on
/// `user_group.source`, mirroring the `ListPermission` idiom.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum GroupSource {
    /// Members are added and removed by the owner. The default.
    Manual = 0,
    /// Created from a Discord guild. The guild link supplies the group's
    /// identity (name, icon); membership is still owner-managed.
    DiscordGuild = 1,
    /// At least one Discord role is imported, so sync owns part of the
    /// membership.
    DiscordGuildMirrored = 2,
}

impl From<i16> for GroupSource {
    fn from(value: i16) -> Self {
        match value {
            1 => GroupSource::DiscordGuild,
            2 => GroupSource::DiscordGuildMirrored,
            _ => GroupSource::Manual,
        }
    }
}

/// Who put a member into a group. Sync only ever removes `Synced` members.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum GroupMemberSource {
    /// Added by the owner, redeemed an invite, or created the group.
    Manual = 0,
    /// Added by Discord reconciliation or a gateway event.
    Synced = 1,
}

impl From<i16> for GroupMemberSource {
    fn from(value: i16) -> Self {
        match value {
            1 => GroupMemberSource::Synced,
            _ => GroupMemberSource::Manual,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum GroupRoleSource {
    Manual = 0,
    DiscordRole = 1,
}

impl From<i16> for GroupRoleSource {
    fn from(value: i16) -> Self {
        match value {
            1 => GroupRoleSource::DiscordRole,
            _ => GroupRoleSource::Manual,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum GroupRoleSyncState {
    Synced = 0,
    /// The Discord role or guild behind this role no longer exists. Members
    /// are kept; nothing will update them again.
    Orphaned = 1,
}

impl From<i16> for GroupRoleSyncState {
    fn from(value: i16) -> Self {
        match value {
            0 => GroupRoleSyncState::Synced,
            _ => GroupRoleSyncState::Orphaned,
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
    /// Set when the bot was removed from the guild. The group keeps its
    /// members but is no longer linked to Discord.
    pub frozen_reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct GroupRole {
    pub id: i32,
    pub group_id: i32,
    pub name: String,
    pub discord_role_id: Option<i64>,
    pub source: GroupRoleSource,
    pub sync_state: GroupRoleSyncState,
    pub last_synced_at: Option<DateTime<Utc>>,
    pub position: i32,
    pub member_count: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CreateGroupRole {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RenameGroupRole {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ImportDiscordRole {
    pub discord_role_id: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserGroupMember {
    pub group_id: i32,
    pub user_id: i64,
    pub username: String,
    pub source: GroupMemberSource,
    /// Ids of the `GroupRole`s this member holds within the group.
    pub roles: Vec<i32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CreateGroup {
    pub name: String,
}

/// Optional body for adding a member by id. `display_name` lets the server
/// create the user's row when they have never logged in.
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct AddGroupMember {
    #[serde(default)]
    pub display_name: Option<String>,
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
        for source in [
            GroupSource::Manual,
            GroupSource::DiscordGuild,
            GroupSource::DiscordGuildMirrored,
        ] {
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

    #[test]
    fn group_source_mirrored_round_trips() {
        assert_eq!(GroupSource::from(2), GroupSource::DiscordGuildMirrored);
        assert_eq!(GroupSource::DiscordGuildMirrored as i16, 2);
    }

    #[test]
    fn member_source_round_trips_and_defaults_to_manual() {
        use super::GroupMemberSource;
        assert_eq!(GroupMemberSource::from(1), GroupMemberSource::Synced);
        assert_eq!(GroupMemberSource::from(0), GroupMemberSource::Manual);
        // Unknown values degrade to Manual: sync must never gain the right to
        // remove a member it does not positively know it added.
        assert_eq!(GroupMemberSource::from(7), GroupMemberSource::Manual);
    }

    #[test]
    fn role_enums_round_trip() {
        use super::{GroupRoleSource, GroupRoleSyncState};
        assert_eq!(GroupRoleSource::from(1), GroupRoleSource::DiscordRole);
        assert_eq!(GroupRoleSource::from(0), GroupRoleSource::Manual);
        assert_eq!(GroupRoleSource::from(9), GroupRoleSource::Manual);
        assert_eq!(GroupRoleSyncState::from(1), GroupRoleSyncState::Orphaned);
        assert_eq!(GroupRoleSyncState::from(0), GroupRoleSyncState::Synced);
        // An unknown state is treated as orphaned so a future value never
        // makes the UI claim a role is healthy.
        assert_eq!(GroupRoleSyncState::from(9), GroupRoleSyncState::Orphaned);
    }

    #[test]
    fn group_role_serialises_last_synced_as_rfc3339_or_null() {
        use super::{GroupRole, GroupRoleSource, GroupRoleSyncState};
        let role = GroupRole {
            id: 1,
            group_id: 2,
            name: "Officers".to_string(),
            discord_role_id: Some(123),
            source: GroupRoleSource::DiscordRole,
            sync_state: GroupRoleSyncState::Synced,
            last_synced_at: None,
            position: 3,
            member_count: 4,
        };
        let json = serde_json::to_string(&role).unwrap();
        assert!(json.contains(r#""last_synced_at":null"#));
        let back: GroupRole = serde_json::from_str(&json).unwrap();
        assert_eq!(back, role);
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
