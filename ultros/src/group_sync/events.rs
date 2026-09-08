//! Gateway handlers for Discord membership sync.
//!
//! These are a latency optimization, not the source of truth — reconciliation
//! is (see [`super::reconcile`]). Everything here applies the same idempotent
//! plans through the same DB primitive reconciliation uses, so an event and a
//! reconcile racing on one guild land on the same state either way, and a
//! missed event costs freshness rather than correctness.
//!
//! Each function takes the *parsed fields* of an event rather than a live
//! serenity `Context`, so the policy is testable against a database fixture
//! without a gateway connection. `discord/mod.rs` does the unwrapping.

use std::collections::HashSet;

use anyhow::Result;
use tracing::debug;
use ultros_db::UltrosDb;
use ultros_db::group_roles::{RoleSyncPlan, SyncSummary};

use super::diff;

/// Stored on the group when the bot is removed from its guild. Non-null is
/// what drives the frozen banner; the text is the fallback the UI shows if it
/// has nothing better to say.
pub(crate) const BOT_REMOVED_REASON: &str = "The Ultros bot is no longer in the linked Discord server, so this group's membership \
     is no longer synced. Members were kept and are now managed by hand.";

/// Apply plans that are already grouped by group, summing what happened.
async fn apply(db: &UltrosDb, plans: Vec<(i32, Vec<RoleSyncPlan>)>) -> Result<SyncSummary> {
    let mut total = SyncSummary::default();
    for (group_id, plans) in plans {
        let summary = db.apply_role_sync(group_id, plans).await?;
        total.added += summary.added;
        total.removed += summary.removed;
        total.left_group += summary.left_group;
        total.handed_over += summary.handed_over;
    }
    Ok(total)
}

/// A member joined the guild, or their roles changed.
///
/// `role_ids` is Discord's own list for the member, which never contains
/// `@everyone`; being in the guild at all is what holding `@everyone` means,
/// so it is added here rather than expected from Discord.
///
/// Handles both `GuildMemberAddition` and `GuildMemberUpdate` because the two
/// want the same thing: make the member's synced roles match what Discord
/// says they hold right now. Doing it by absolute state rather than by delta
/// is what makes replaying an event harmless.
pub(crate) async fn on_member_upsert(
    db: &UltrosDb,
    guild_id: i64,
    user_id: i64,
    display_name: &str,
    role_ids: &[i64],
) -> Result<SyncSummary> {
    // The one indexed early-out. Most guilds the bot is in have no synced
    // roles at all, and for those this is the entire cost of the event.
    let roles = db.synced_roles_for_guild(guild_id).await?;
    if roles.is_empty() {
        return Ok(SyncSummary::default());
    }
    let held: HashSet<i64> = role_ids.iter().copied().collect();
    let plans = diff::member_plans(&roles, user_id, display_name, |discord_role_id| {
        diff::is_everyone_role(guild_id, discord_role_id) || held.contains(&discord_role_id)
    });
    apply(db, plans).await
}

/// A member left the guild, was kicked, or was banned: they hold nothing now.
///
/// Their group membership goes too, but only if sync is what put them there —
/// that decision belongs to `apply_role_sync`, which never removes a member
/// the owner added or who redeemed an invite.
pub(crate) async fn on_member_removal(
    db: &UltrosDb,
    guild_id: i64,
    user_id: i64,
) -> Result<SyncSummary> {
    let roles = db.synced_roles_for_guild(guild_id).await?;
    if roles.is_empty() {
        return Ok(SyncSummary::default());
    }
    // The display name is only ever used for adds, and there are none here.
    let plans = diff::member_plans(&roles, user_id, "", |_| false);
    apply(db, plans).await
}

/// A Discord role was deleted. The imported role keeps its members and stops
/// updating, so a list shared to it does not silently lose its audience.
///
/// `mark_role_orphaned` opens with a single indexed lookup and returns without
/// writing when the guild has no such role, so it is its own early-out.
pub(crate) async fn on_role_delete(
    db: &UltrosDb,
    guild_id: i64,
    discord_role_id: i64,
) -> Result<()> {
    db.mark_role_orphaned(guild_id, discord_role_id).await
}

/// The bot left a guild, or the guild went offline.
///
/// `unavailable` separates the two, and the difference matters a great deal:
/// an outage is temporary and must change nothing, while a kick means the
/// group can never sync again and has to be frozen. Freezing keeps every
/// member — deleting the group would cascade through `list_shared_group` and
/// silently revoke every list share pointing at it.
pub(crate) async fn on_guild_delete(
    db: &UltrosDb,
    guild_id: i64,
    unavailable: bool,
) -> Result<Option<i32>> {
    if unavailable {
        debug!(guild_id, "guild is unavailable, leaving its group alone");
        return Ok(None);
    }
    db.freeze_group_for_guild(guild_id, BOT_REMOVED_REASON.to_string())
        .await
}

/// The bot connected to a guild. Nothing to do: this replays on every
/// reconnect, and reconciliation already covers whatever changed while the
/// bot was away.
pub(crate) fn on_guild_create(guild_id: i64) {
    debug!(guild_id, "guild available");
}

/// Live-DB tests. Run with a disposable database:
///
/// ```bash
/// cargo test -p ultros group_sync::events -- --ignored --test-threads=1
/// ```
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicI64, Ordering};
    use ultros_api_types::user::group::{GroupMemberSource, GroupRoleSyncState};

    static NEXT_ID: AtomicI64 = AtomicI64::new(0);

    /// Ids start from the current time so re-running against the same database
    /// never collides with a previous run's rows.
    fn next_id() -> i64 {
        let base = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros() as i64;
        base + NEXT_ID.fetch_add(1, Ordering::Relaxed)
    }

    async fn test_db() -> UltrosDb {
        UltrosDb::connect().await.expect("connect to test DB")
    }

    /// A guild-linked group with one imported role, plus the ids the event
    /// handlers take: `(db, group_id, owner_id, role_id, guild_id,
    /// discord_role_id)`.
    async fn synced_group(db: &UltrosDb) -> (i32, i64, i32, i64, i64) {
        let owner_id = next_id();
        let owner = db
            .get_or_create_discord_user(owner_id as u64, format!("owner-{owner_id}"))
            .await
            .unwrap();
        let guild_id = next_id();
        let group = db
            .create_group_from_guild("Sync events group".to_string(), owner.id, guild_id, None)
            .await
            .unwrap();
        let discord_role_id = next_id();
        let role = db
            .import_discord_role(
                group.id,
                owner.id,
                discord_role_id,
                "Raiders".to_string(),
                1,
            )
            .await
            .unwrap();
        (group.id, owner.id, role.id, guild_id, discord_role_id)
    }

    async fn member_source(
        db: &UltrosDb,
        group_id: i32,
        owner_id: i64,
        user_id: i64,
    ) -> Option<i16> {
        db.get_group_members(group_id, owner_id)
            .await
            .unwrap()
            .into_iter()
            .find(|member| member.0.user_id == user_id)
            .map(|member| member.0.source)
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn joining_with_a_synced_role_adds_the_member_to_the_group() {
        let db = test_db().await;
        let (group_id, owner_id, role_id, guild_id, discord_role_id) = synced_group(&db).await;
        let joiner = next_id();

        let summary = on_member_upsert(&db, guild_id, joiner, "Joiner", &[discord_role_id])
            .await
            .unwrap();

        assert_eq!(summary.added, 1);
        assert!(db.role_member_ids(role_id).await.unwrap().contains(&joiner));
        assert_eq!(
            member_source(&db, group_id, owner_id, joiner).await,
            Some(GroupMemberSource::Synced as i16)
        );
    }

    /// The same event twice must land in the same place, because the gateway
    /// can redeliver and a reconcile may have already applied it.
    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn replaying_a_join_changes_nothing() {
        let db = test_db().await;
        let (_group_id, _owner_id, role_id, guild_id, discord_role_id) = synced_group(&db).await;
        let joiner = next_id();

        on_member_upsert(&db, guild_id, joiner, "Joiner", &[discord_role_id])
            .await
            .unwrap();
        let second = on_member_upsert(&db, guild_id, joiner, "Joiner", &[discord_role_id])
            .await
            .unwrap();

        assert_eq!(second.added, 1, "the upsert re-affirms rather than errors");
        assert_eq!(
            db.role_member_ids(role_id).await.unwrap().len(),
            1,
            "and does not duplicate the membership"
        );
    }

    /// A member who was also added by hand keeps their group membership and
    /// only loses the role chip. This is the case the whole `source` column
    /// exists for.
    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn losing_a_discord_role_never_evicts_a_manual_member() {
        let db = test_db().await;
        let (group_id, owner_id, role_id, guild_id, discord_role_id) = synced_group(&db).await;
        let member = next_id();
        on_member_upsert(&db, guild_id, member, "Member", &[discord_role_id])
            .await
            .unwrap();
        // The owner adds them by hand as well, which promotes the membership.
        db.add_group_member(group_id, owner_id, member, Some("Member".to_string()))
            .await
            .unwrap();

        on_member_upsert(&db, guild_id, member, "Member", &[])
            .await
            .unwrap();

        assert!(!db.role_member_ids(role_id).await.unwrap().contains(&member));
        assert_eq!(
            member_source(&db, group_id, owner_id, member).await,
            Some(GroupMemberSource::Manual as i16),
            "a manually added member stays in the group"
        );
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn leaving_the_guild_removes_a_synced_member_entirely() {
        let db = test_db().await;
        let (group_id, owner_id, role_id, guild_id, discord_role_id) = synced_group(&db).await;
        let member = next_id();
        on_member_upsert(&db, guild_id, member, "Member", &[discord_role_id])
            .await
            .unwrap();

        let summary = on_member_removal(&db, guild_id, member).await.unwrap();

        assert_eq!(summary.left_group, 1);
        assert!(!db.role_member_ids(role_id).await.unwrap().contains(&member));
        assert_eq!(member_source(&db, group_id, owner_id, member).await, None);
    }

    /// An event for a guild with nothing synced must cost one query and stop.
    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn an_unsynced_guild_is_a_no_op() {
        let db = test_db().await;
        let summary = on_member_upsert(&db, next_id(), next_id(), "Nobody", &[next_id()])
            .await
            .unwrap();
        assert_eq!(summary, SyncSummary::default());

        on_member_removal(&db, next_id(), next_id()).await.unwrap();
        on_role_delete(&db, next_id(), next_id()).await.unwrap();
    }

    /// Whole-server sync: `@everyone` carries the guild's id and appears in
    /// nobody's role list, so a member holding no roles at all still belongs.
    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn everyone_takes_in_a_member_with_no_roles() {
        let db = test_db().await;
        let owner_id = next_id();
        let owner = db
            .get_or_create_discord_user(owner_id as u64, format!("owner-{owner_id}"))
            .await
            .unwrap();
        let guild_id = next_id();
        let group = db
            .create_group_from_guild("Whole server".to_string(), owner.id, guild_id, None)
            .await
            .unwrap();
        // Importing `@everyone` is importing the role whose id is the guild's.
        let role = db
            .import_discord_role(group.id, owner.id, guild_id, "@everyone".to_string(), 0)
            .await
            .unwrap();
        let member = next_id();

        on_member_upsert(&db, guild_id, member, "Member", &[])
            .await
            .unwrap();

        assert!(db.role_member_ids(role.id).await.unwrap().contains(&member));
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn deleting_the_discord_role_orphans_it_and_keeps_its_members() {
        let db = test_db().await;
        let (group_id, owner_id, role_id, guild_id, discord_role_id) = synced_group(&db).await;
        let member = next_id();
        on_member_upsert(&db, guild_id, member, "Member", &[discord_role_id])
            .await
            .unwrap();

        on_role_delete(&db, guild_id, discord_role_id)
            .await
            .unwrap();

        let role = db
            .get_group_roles(group_id, owner_id)
            .await
            .unwrap()
            .into_iter()
            .find(|role| role.0.id == role_id)
            .unwrap();
        assert_eq!(role.0.sync_state, GroupRoleSyncState::Orphaned as i16);
        assert_eq!(role.1, 1, "members are kept");
        // ...and the guild no longer appears in a reconcile cycle.
        assert!(
            !db.guilds_with_synced_roles()
                .await
                .unwrap()
                .contains(&guild_id)
        );
    }

    /// An outage is not a kick. Freezing on `unavailable` would unlink groups
    /// every time Discord had a bad day.
    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn an_unavailable_guild_is_left_alone() {
        let db = test_db().await;
        let (_group_id, _owner_id, _role_id, guild_id, _discord_role_id) = synced_group(&db).await;

        assert_eq!(on_guild_delete(&db, guild_id, true).await.unwrap(), None);

        assert!(
            db.guilds_with_synced_roles()
                .await
                .unwrap()
                .contains(&guild_id)
        );
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn being_removed_from_the_guild_freezes_the_group_and_keeps_members() {
        let db = test_db().await;
        let (group_id, owner_id, _role_id, guild_id, discord_role_id) = synced_group(&db).await;
        let member = next_id();
        on_member_upsert(&db, guild_id, member, "Member", &[discord_role_id])
            .await
            .unwrap();

        assert_eq!(
            on_guild_delete(&db, guild_id, false).await.unwrap(),
            Some(group_id)
        );

        let (group, _roles, member_count) = db.get_group_detail(group_id, owner_id).await.unwrap();
        assert!(group.guild_id.is_none(), "the guild slot is released");
        assert!(group.frozen_reason.is_some());
        assert_eq!(member_count, 2, "owner and synced member both stay");
        assert!(
            !db.guilds_with_synced_roles()
                .await
                .unwrap()
                .contains(&guild_id)
        );
    }
}
