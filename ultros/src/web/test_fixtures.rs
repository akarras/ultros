//! Test-only group fixtures.
//!
//! Compile-gated behind `test-auth` exactly like [`super::oauth::test_auth`]:
//! prod Docker builds don't pass `--features test-auth`, so none of this is in
//! the binary.
//!
//! Why it exists: two of the three states `/groups/:id` renders differently —
//! guild-linked/mirrored and frozen — are only reachable through a live
//! Discord guild and the gateway's guild-removal handler. The E2E harness has
//! neither, so the group page could only ever be driven in its manual state.
//! This route reaches those states through the same [`UltrosDb`] calls the
//! Discord paths use, with ids the test makes up, so the browser sees rows the
//! real code would have written rather than a hand-built approximation.

use axum::{Json, Router, extract::State, routing::post};
use serde::{Deserialize, Serialize};
use ultros_db::{UltrosDb, group_roles::RoleSyncPlan};

use crate::web::error::ApiError;
use crate::web::oauth::AuthDiscordUser;
use crate::web::state::WebState;

/// A member reconciliation should be pretended to have found in a role.
#[derive(Debug, Deserialize)]
pub struct FixtureRoleMember {
    pub user_id: i64,
    pub display_name: String,
}

/// A Discord role to import into the fixture group, plus who holds it.
#[derive(Debug, Deserialize)]
pub struct FixtureDiscordRole {
    pub discord_role_id: i64,
    pub name: String,
    /// Discord's own ordering. Irrelevant to a single-role fixture, so it
    /// defaults rather than making every caller pick a number.
    #[serde(default)]
    pub position: i32,
    /// Applied through the real sync path, so these land as `Synced` group
    /// members holding the role and move the role's `last_synced_at`.
    #[serde(default)]
    pub members: Vec<FixtureRoleMember>,
    /// Mark the role orphaned once its members are in, as the gateway does
    /// when the Discord role behind it is deleted while the guild link
    /// survives. Sync skips an orphaned role, so this is applied after the
    /// members, never before.
    #[serde(default)]
    pub orphaned: bool,
}

/// A group to build, in whatever state the caller needs it.
///
/// The three states of the group page fall out of which fields are set:
/// nothing but a name gives a manual group, a `guild_id` with roles gives a
/// Discord-mirrored one, and adding `frozen_reason` freezes that.
#[derive(Debug, Deserialize)]
pub struct GroupFixture {
    pub name: String,
    /// Set to link the group to a Discord guild. Any id will do — nothing
    /// here talks to Discord — but it must not already be taken by another
    /// group, so tests should derive it from their own unique seed.
    #[serde(default)]
    pub guild_id: Option<i64>,
    #[serde(default)]
    pub guild_icon_url: Option<String>,
    #[serde(default)]
    pub discord_roles: Vec<FixtureDiscordRole>,
    /// Applied last, through the same call the gateway makes when the bot is
    /// removed from a guild: the guild link is released, imported roles are
    /// orphaned, and synced members become the owner's to manage.
    #[serde(default)]
    pub frozen_reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GroupFixtureResult {
    pub group_id: i32,
    /// Ids of the imported roles, in the order they were given.
    pub role_ids: Vec<i32>,
}

/// `POST /test/group/fixture` — build a group for the caller in one round trip.
pub async fn create_group_fixture(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Json(fixture): Json<GroupFixture>,
) -> Result<Json<GroupFixtureResult>, ApiError> {
    let owner_id = user.id as i64;
    let group = match fixture.guild_id {
        Some(guild_id) => {
            db.create_group_from_guild(fixture.name, owner_id, guild_id, fixture.guild_icon_url)
                .await?
        }
        None => db.create_group(fixture.name, owner_id).await?,
    };

    let mut role_ids = Vec::with_capacity(fixture.discord_roles.len());
    let mut plans = Vec::with_capacity(fixture.discord_roles.len());
    let mut orphan_after_sync = Vec::new();
    for role in fixture.discord_roles {
        let imported = db
            .import_discord_role(
                group.id,
                owner_id,
                role.discord_role_id,
                role.name,
                role.position,
            )
            .await?;
        role_ids.push(imported.id);
        if role.orphaned {
            orphan_after_sync.push(role.discord_role_id);
        }
        plans.push(RoleSyncPlan {
            role_id: imported.id,
            adds: role
                .members
                .into_iter()
                .map(|member| (member.user_id, member.display_name))
                .collect(),
            removes: Vec::new(),
        });
    }
    if !plans.is_empty() {
        db.apply_role_sync(group.id, plans).await?;
    }
    // `import_discord_role` already refused a group with no guild link, so
    // reaching here with an orphan to mark means there is one.
    if let Some(guild_id) = group.guild_id {
        for discord_role_id in orphan_after_sync {
            db.mark_role_orphaned(guild_id, discord_role_id).await?;
        }
    }

    if let Some(reason) = fixture.frozen_reason {
        // Freezing is keyed by guild, because that is what the gateway event
        // carries. A group that was never linked has nothing to freeze.
        let guild_id = group.guild_id.ok_or(ApiError::BadRequest(
            "Only a guild-linked group can be frozen",
        ))?;
        db.freeze_group_for_guild(guild_id, reason).await?;
    }

    Ok(Json(GroupFixtureResult {
        group_id: group.id,
        role_ids,
    }))
}

pub fn routes() -> Router<WebState> {
    Router::new().route("/test/group/fixture", post(create_group_fixture))
}
