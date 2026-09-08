//! The pure half of Discord membership sync: given what Discord says a role's
//! members are and what we currently have, decide what changes.
//!
//! Nothing in here does IO, which is the point — reconciliation and the
//! gateway handlers both funnel through these functions, so the two paths
//! cannot disagree about what "in sync" means, and the interesting cases are
//! testable without a database or a Discord connection.

use std::collections::{BTreeMap, HashSet};
use ultros_db::group_roles::{RoleSyncPlan, SyncedRole};

/// One guild member as reconciliation sees them: who they are, what to call
/// them, and which Discord roles they hold.
///
/// `role_ids` is Discord's own list, which never includes `@everyone` — see
/// [`is_everyone_role`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GuildMember {
    pub user_id: i64,
    pub display_name: String,
    pub role_ids: Vec<i64>,
}

/// Discord gives `@everyone` the guild's own id and omits it from every
/// member's role list, so "does this member hold the role" has to special-case
/// it. Importing `@everyone` is how the spec expresses "sync the whole
/// server", so this is a load-bearing case rather than a curiosity.
pub(crate) fn is_everyone_role(guild_id: i64, discord_role_id: i64) -> bool {
    guild_id == discord_role_id
}

/// The member set a synced role should have: everyone holding the Discord
/// role, or every member of the guild when the role is `@everyone`.
///
/// A `BTreeMap` rather than a `HashMap` so the resulting adds come out in a
/// stable order — the summary log and the tests both read better for it, and
/// a deterministic statement order inside one transaction is one less way for
/// two concurrent syncs to deadlock against each other.
pub(crate) fn desired_members(
    guild_id: i64,
    discord_role_id: i64,
    members: &[GuildMember],
) -> BTreeMap<i64, String> {
    let everyone = is_everyone_role(guild_id, discord_role_id);
    members
        .iter()
        .filter(|member| everyone || member.role_ids.contains(&discord_role_id))
        .map(|member| (member.user_id, member.display_name.clone()))
        .collect()
}

/// What one role needs changed to match Discord.
///
/// Adds are the desired members we do not already have; removes are the
/// members we have that Discord no longer lists. Members present in both are
/// left alone — the display-name refresh for those is a separate bulk update
/// in [`super::reconcile`], because re-adding thousands of unchanged rows
/// through the per-row upsert path would make every cycle on a large guild
/// hold a long transaction for no membership change.
pub(crate) fn plan(
    role_id: i32,
    desired: &BTreeMap<i64, String>,
    current: &HashSet<i64>,
) -> RoleSyncPlan {
    let adds = desired
        .iter()
        .filter(|(user_id, _)| !current.contains(user_id))
        .map(|(user_id, name)| (*user_id, name.clone()))
        .collect();
    let mut removes: Vec<i64> = current
        .iter()
        .copied()
        .filter(|user_id| !desired.contains_key(user_id))
        .collect();
    removes.sort_unstable();
    RoleSyncPlan {
        role_id,
        adds,
        removes,
    }
}

/// The plans for a *single* member across every synced role of a guild,
/// grouped by the group each role belongs to (which is what
/// `apply_role_sync` is keyed on).
///
/// `holds` answers "does this member hold that Discord role", which lets the
/// same function serve both gateway cases: a member who is still in the guild
/// passes a predicate over their current role ids, and a member who left
/// passes one that is always false, removing them from everything.
///
/// Every synced role gets a plan, including the ones with nothing to do. An
/// empty plan is not wasted work: `apply_role_sync` stamps `last_synced_at` on
/// every role it is handed, which is what tells the UI the role is live.
pub(crate) fn member_plans(
    roles: &[SyncedRole],
    user_id: i64,
    display_name: &str,
    holds: impl Fn(i64) -> bool,
) -> Vec<(i32, Vec<RoleSyncPlan>)> {
    let mut by_group: BTreeMap<i32, Vec<RoleSyncPlan>> = BTreeMap::new();
    for role in roles {
        let (adds, removes) = if holds(role.discord_role_id) {
            (vec![(user_id, display_name.to_string())], Vec::new())
        } else {
            (Vec::new(), vec![user_id])
        };
        by_group
            .entry(role.group_id)
            .or_default()
            .push(RoleSyncPlan {
                role_id: role.role_id,
                adds,
                removes,
            });
    }
    by_group.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(user_id: i64, name: &str, role_ids: &[i64]) -> GuildMember {
        GuildMember {
            user_id,
            display_name: name.to_string(),
            role_ids: role_ids.to_vec(),
        }
    }

    fn desired(pairs: &[(i64, &str)]) -> BTreeMap<i64, String> {
        pairs
            .iter()
            .map(|(id, name)| (*id, name.to_string()))
            .collect()
    }

    fn current(ids: &[i64]) -> HashSet<i64> {
        ids.iter().copied().collect()
    }

    fn synced(role_id: i32, group_id: i32, discord_role_id: i64) -> SyncedRole {
        SyncedRole {
            role_id,
            group_id,
            discord_role_id,
        }
    }

    #[test]
    fn plans_cover_adds_removes_and_no_ops() {
        struct Case {
            name: &'static str,
            desired: Vec<(i64, &'static str)>,
            current: Vec<i64>,
            adds: Vec<(i64, &'static str)>,
            removes: Vec<i64>,
        }

        let cases = [
            Case {
                name: "a brand new role takes on every desired member",
                desired: vec![(1, "ann"), (2, "bob")],
                current: vec![],
                adds: vec![(1, "ann"), (2, "bob")],
                removes: vec![],
            },
            Case {
                name: "a member who lost the Discord role is removed",
                desired: vec![(1, "ann")],
                current: vec![1, 2],
                adds: vec![],
                removes: vec![2],
            },
            Case {
                name: "an unchanged role produces nothing at all",
                desired: vec![(1, "ann"), (2, "bob")],
                current: vec![1, 2],
                adds: vec![],
                removes: vec![],
            },
            Case {
                name: "adds and removes can happen in the same pass",
                desired: vec![(1, "ann"), (3, "cat")],
                current: vec![1, 2],
                adds: vec![(3, "cat")],
                removes: vec![2],
            },
            Case {
                name: "an emptied Discord role removes everyone",
                desired: vec![],
                current: vec![1, 2, 3],
                adds: vec![],
                removes: vec![1, 2, 3],
            },
        ];

        for case in cases {
            let got = plan(7, &desired(&case.desired), &current(&case.current));
            assert_eq!(got.role_id, 7, "{}", case.name);
            let expected_adds: Vec<(i64, String)> = case
                .adds
                .iter()
                .map(|(id, name)| (*id, name.to_string()))
                .collect();
            assert_eq!(got.adds, expected_adds, "{}", case.name);
            assert_eq!(got.removes, case.removes, "{}", case.name);
        }
    }

    /// Applying the same plan twice must be a no-op the second time round,
    /// which is what lets a reconcile and a gateway event race without the
    /// end state depending on who won.
    #[test]
    fn a_plan_applied_once_leaves_nothing_to_do() {
        let want = desired(&[(1, "ann"), (2, "bob")]);
        let first = plan(7, &want, &current(&[]));
        assert_eq!(first.adds.len(), 2);
        let after: HashSet<i64> = first.adds.iter().map(|(id, _)| *id).collect();
        let second = plan(7, &want, &after);
        assert!(second.adds.is_empty());
        assert!(second.removes.is_empty());
    }

    /// A refreshed display name for someone already in the role is not an
    /// add. Names are refreshed in bulk by reconciliation instead, so this
    /// pass stays proportional to what actually changed.
    #[test]
    fn a_renamed_member_is_not_re_added() {
        let got = plan(7, &desired(&[(1, "ann the second")]), &current(&[1]));
        assert!(got.adds.is_empty());
        assert!(got.removes.is_empty());
    }

    #[test]
    fn the_everyone_role_is_the_guilds_own_id() {
        assert!(is_everyone_role(100, 100));
        assert!(!is_everyone_role(100, 200));
    }

    /// `@everyone` means the whole server, even though Discord lists it in
    /// nobody's roles.
    #[test]
    fn everyone_wants_every_member_regardless_of_their_roles() {
        let members = [
            member(1, "ann", &[500]),
            member(2, "bob", &[]),
            member(3, "cat", &[500, 501]),
        ];
        assert_eq!(
            desired_members(100, 100, &members),
            desired(&[(1, "ann"), (2, "bob"), (3, "cat")])
        );
    }

    #[test]
    fn an_ordinary_role_wants_only_the_members_holding_it() {
        let members = [
            member(1, "ann", &[500]),
            member(2, "bob", &[]),
            member(3, "cat", &[500, 501]),
        ];
        assert_eq!(
            desired_members(100, 500, &members),
            desired(&[(1, "ann"), (3, "cat")])
        );
        assert_eq!(desired_members(100, 501, &members), desired(&[(3, "cat")]));
        assert_eq!(desired_members(100, 999, &members), desired(&[]));
    }

    /// The whole-server case on an empty guild must produce an empty desired
    /// set, not "leave everything alone" — otherwise a server the bot can no
    /// longer read members for would look identical to a full one.
    #[test]
    fn everyone_on_an_empty_member_list_wants_nobody() {
        assert_eq!(desired_members(100, 100, &[]), desired(&[]));
    }

    #[test]
    fn a_members_plans_add_the_roles_they_hold_and_remove_the_rest() {
        let roles = [synced(1, 10, 500), synced(2, 10, 501), synced(3, 11, 502)];
        let held: HashSet<i64> = [500, 502].into_iter().collect();
        let plans = member_plans(&roles, 42, "ann", |id| held.contains(&id));

        assert_eq!(plans.len(), 2, "one entry per group");
        let (group_10, ref group_10_plans) = plans[0];
        assert_eq!(group_10, 10);
        assert_eq!(group_10_plans[0].adds, vec![(42, "ann".to_string())]);
        assert!(group_10_plans[0].removes.is_empty());
        assert!(group_10_plans[1].adds.is_empty());
        assert_eq!(group_10_plans[1].removes, vec![42]);

        let (group_11, ref group_11_plans) = plans[1];
        assert_eq!(group_11, 11);
        assert_eq!(group_11_plans[0].adds, vec![(42, "ann".to_string())]);
    }

    /// A member who left the guild is removed from every synced role; the
    /// group membership itself is the DB layer's call, and it only lets go of
    /// members sync put there.
    #[test]
    fn a_departed_member_is_removed_from_every_synced_role() {
        let roles = [synced(1, 10, 500), synced(2, 10, 501)];
        let plans = member_plans(&roles, 42, "ann", |_| false);
        assert_eq!(plans.len(), 1);
        assert!(plans[0].1.iter().all(|plan| plan.adds.is_empty()));
        assert_eq!(
            plans[0]
                .1
                .iter()
                .map(|p| p.removes.clone())
                .collect::<Vec<_>>(),
            vec![vec![42], vec![42]]
        );
    }

    /// Every synced role is represented even when the member changes nothing
    /// about it, because the apply step stamps `last_synced_at` on each role
    /// it is handed.
    #[test]
    fn every_synced_role_gets_a_plan() {
        let roles = [synced(1, 10, 500), synced(2, 10, 501), synced(3, 10, 502)];
        let plans = member_plans(&roles, 42, "ann", |id| id == 501);
        assert_eq!(plans[0].1.len(), 3);
    }
}
