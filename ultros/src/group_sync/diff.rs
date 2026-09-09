//! The pure half of Discord membership sync: given what Discord says a role's
//! members are and what we currently have, decide what changes.
//!
//! Nothing in here does IO, which is the point — reconciliation and the
//! gateway handlers both funnel through these functions, so the two paths
//! cannot disagree about what "in sync" means, and the interesting cases are
//! testable without a database or a Discord connection.
//!
//! [`plan`] and [`desired_members`] are deliberately literal: they say what
//! the snapshot they were handed implies, including "remove everybody" when
//! the snapshot is empty. Deciding whether a snapshot is worth believing is a
//! separate job, done by [`check_member_list`] and [`check_removals`], which
//! reconciliation calls before it applies anything.

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use ultros_db::group_roles::{RoleSyncPlan, SyncedRole};

/// A role holding fewer members than this is exempt from
/// [`check_removals`]: at that size a percentage says nothing. A three-person
/// role losing two people is 67% and an entirely ordinary Tuesday.
pub(crate) const REMOVAL_BREAKER_MIN_MEMBERS: usize = 10;

/// The share of a role's current members a single reconcile may remove before
/// the snapshot is treated as wrong rather than the guild as emptied.
///
/// Fifty percent, because of what the two failure modes cost. Gateway events
/// already handle ordinary churn one member at a time, so by the time a
/// reconcile runs, `current` has usually shrunk alongside Discord and the
/// plan's removes are a handful of stragglers. A reconcile that suddenly wants
/// to drop half a role is therefore far more likely to be reading a bad
/// snapshot — a truncated page, an intent that answers empty instead of 403,
/// a proxy that returned a partial body — than to be watching an exodus that
/// every gateway event somehow missed. Refusing costs stale membership until
/// the next pass, which is recoverable; proceeding revokes list access for
/// half a guild, which is not.
pub(crate) const REMOVAL_BREAKER_PERCENT: usize = 50;

/// Why a computed plan must not be applied.
///
/// Every variant means the same thing to the caller: throw the whole guild's
/// plan away and leave membership exactly as it is. A guild we could not read
/// properly is not a guild that emptied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UnsafePlan {
    /// Discord answered with no members at all.
    EmptyMemberList,
    /// One role would lose an implausible share of its members at once.
    MassRemoval {
        role_id: i32,
        removes: usize,
        current: usize,
    },
}

impl fmt::Display for UnsafePlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyMemberList => write!(
                f,
                "Discord listed no members for this guild, which cannot be true — the bot is \
                 itself a member. Treating the response as bad rather than as an emptied server"
            ),
            Self::MassRemoval {
                role_id,
                removes,
                current,
            } => write!(
                f,
                "reconciling role {role_id} would remove {removes} of its {current} members, over \
                 the {REMOVAL_BREAKER_PERCENT}% circuit breaker. Refusing the whole guild's plan; \
                 membership is unchanged"
            ),
        }
    }
}

impl std::error::Error for UnsafePlan {}

/// Refuse to plan against a member list that cannot be real.
///
/// A guild always has at least one member, because the bot reading it is one.
/// An empty 200 is therefore never "the server emptied", it is a Discord
/// incident, a proxy oddity, or an intent state answering empty instead of
/// 403 — and [`plan`] would faithfully turn it into "remove everybody".
pub(crate) fn check_member_list(members: &[GuildMember]) -> Result<(), UnsafePlan> {
    if members.is_empty() {
        return Err(UnsafePlan::EmptyMemberList);
    }
    Ok(())
}

/// Refuse a plan that would remove an implausible share of one role.
///
/// This is the backstop for the failures [`check_member_list`] cannot see: a
/// member list that is short rather than empty. See
/// [`REMOVAL_BREAKER_PERCENT`] for why the line is where it is.
///
/// Tripping is deliberately loud and deliberately sticky. If a role really did
/// lose most of its members without the gateway noticing, reconciliation stays
/// refused until a human looks — deleting and re-importing the role, or
/// trimming it by hand, clears it. That is the intended trade: a stuck sync is
/// visible and reversible, a mass eviction is neither.
pub(crate) fn check_removals(
    role_id: i32,
    removes: usize,
    current: usize,
) -> Result<(), UnsafePlan> {
    if current >= REMOVAL_BREAKER_MIN_MEMBERS && removes * 100 > current * REMOVAL_BREAKER_PERCENT {
        return Err(UnsafePlan::MassRemoval {
            role_id,
            removes,
            current,
        });
    }
    Ok(())
}

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
            // What `plan` says, not what reconciliation does with it:
            // `check_removals` refuses a plan this shape before it is applied.
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

    /// An empty member list is refused outright rather than planned against.
    ///
    /// This replaces a test that asserted the opposite — that `@everyone` on
    /// an empty list wants nobody, which reconciliation then applied as
    /// "remove every synced member of the group". A guild can never actually
    /// have zero members: the bot reading it is one. So the empty response is
    /// always a failure to read, and the only safe reading of it is "do not
    /// plan".
    ///
    /// `desired_members` itself stays pure and still answers "nobody" — the
    /// refusal lives one layer up, in the guard reconciliation calls before it
    /// plans anything.
    #[test]
    fn an_empty_member_list_is_refused_instead_of_removing_everyone() {
        assert_eq!(
            check_member_list(&[]),
            Err(UnsafePlan::EmptyMemberList),
            "an empty guild member list must never produce a plan"
        );
        assert_eq!(
            check_member_list(&[member(1, "ann", &[])]),
            Ok(()),
            "a guild with members plans normally"
        );
    }

    /// The circuit breaker for the case the empty check cannot see: a short
    /// member list rather than an absent one.
    #[test]
    fn a_mass_removal_is_refused_and_ordinary_churn_is_not() {
        // Six of ten is over the line, five of ten is not.
        assert_eq!(
            check_removals(7, 6, 10),
            Err(UnsafePlan::MassRemoval {
                role_id: 7,
                removes: 6,
                current: 10,
            })
        );
        assert_eq!(check_removals(7, 5, 10), Ok(()));
        // A whole-server role losing everybody is the exact shape of a
        // truncated response, and is refused however large the role is.
        assert!(check_removals(7, 5_000, 5_000).is_err());
        // Small roles are exempt: at that size a share means nothing.
        assert_eq!(
            check_removals(7, 9, 9),
            Ok(()),
            "a role under the minimum may empty out"
        );
        assert_eq!(check_removals(7, 0, 10_000), Ok(()), "no removes, no worry");
    }

    /// The two guards compose the way reconciliation uses them: an empty
    /// snapshot never even reaches the breaker, and a plausible one that
    /// happens to remove a lot still does.
    #[test]
    fn the_guards_refuse_the_snapshots_that_would_wipe_a_role() {
        let members = [member(1, "ann", &[500])];
        assert!(check_member_list(&members).is_ok());
        // Discord returned one member; the role had a thousand.
        let current: HashSet<i64> = (1..=1000).collect();
        let got = plan(7, &desired_members(100, 500, &members), &current);
        assert_eq!(got.removes.len(), 999);
        assert!(
            check_removals(7, got.removes.len(), current.len()).is_err(),
            "a short member list must not empty the role"
        );
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
