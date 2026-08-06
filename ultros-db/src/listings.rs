use anyhow::Result;
use futures::{Stream, future::try_join_all};
use itertools::Itertools;
use metrics::{counter, histogram};
use migration::DbErr;
use sea_orm::{
    ColumnTrait, DbBackend, EntityTrait, ExprTrait, FromQueryResult, QueryFilter, QuerySelect,
    Statement,
};
use std::{
    collections::{HashMap, HashSet, hash_map::Entry},
    time::Instant,
};
use tracing::instrument;
use ultros_api_types::{ActiveListing, retainer::Retainer};
use universalis::{ItemId, ListingView, WorldId};

use crate::{
    UltrosDb,
    common::partial_diff_iterator::PartialDiffIterator,
    entity::{active_listing, retainer},
};

pub type ListingUpdate = (
    Vec<(ActiveListing, Retainer)>,
    Vec<(ActiveListing, Retainer)>,
);

pub type ListingsWithRetainers = Vec<(active_listing::Model, Option<retainer::Model>)>;

struct ListingData(active_listing::Model, retainer::Model);

/// Sort/compare key for `active_listing::Model`/`retainer::Model` pairs used by
/// `listings_to_remove`: retainer name, price, quantity, hq.
///
/// Deliberately **not** keyed on world. `remove_listings` queries the DB for a
/// single world before diffing, so every row here already shares that world and
/// including it would only add a field for the two sides to disagree about —
/// which is exactly what happened: the key used to lead with world id, and since
/// the websocket never populates it on the incoming side, no DB row ever matched.
fn remove_diff_key_model<'a>(
    listing: &active_listing::Model,
    retainer_name: &'a str,
) -> (&'a str, i32, i32, bool) {
    (
        retainer_name,
        listing.price_per_unit,
        listing.quantity,
        listing.hq,
    )
}

/// Same key, computed from the incoming websocket view.
///
/// Every `Option` here must resolve exactly like the insert path (`create_listing`
/// in lib.rs stores `price_per_unit.unwrap_or(total)` and `quantity.unwrap_or(1)`),
/// otherwise a listing that arrived with a `None` field could never be matched for
/// removal and would linger as a phantom row.
///
/// This is why world id is gone rather than defaulted: Universalis' websocket
/// sends `worldID: null` on every listing in both `listings/add` and
/// `listings/remove` (the world is carried once, on the event envelope). So
/// `world_id.unwrap_or_default()` was always `0` while the DB side held the real
/// world, the tuples could never compare `Equal`, `PartialDiffIterator` never
/// yielded `Same`, and `remove_listings` deleted nothing at all.
fn remove_diff_key_view(listing: &ListingView) -> (&str, i32, i32, bool) {
    (
        listing.retainer_name.as_str(),
        listing.price_per_unit.unwrap_or(listing.total) as i32,
        listing.quantity.unwrap_or(1) as i32,
        listing.hq,
    )
}

// `PartialDiffIterator` drives its merge through these impls; deriving them from
// the same key functions the sorts use makes drift between sort order and merge
// comparator impossible.
impl PartialEq<ListingView> for ListingData {
    fn eq(&self, other: &ListingView) -> bool {
        remove_diff_key_model(&self.0, &self.1.name) == remove_diff_key_view(other)
        // timestamp intentionally ignored
    }
}

impl PartialOrd<ListingView> for ListingData {
    fn partial_cmp(&self, other: &ListingView) -> Option<std::cmp::Ordering> {
        Some(remove_diff_key_model(&self.0, &self.1.name).cmp(&remove_diff_key_view(other)))
    }
}

/// Diffs the DB's current listings against the incoming "listings that no longer
/// exist" view from the websocket, returning the DB rows to delete.
///
/// `PartialDiffIterator` assumes both inputs are already sorted by the same key;
/// duplicate/identical listings are legal (a retainer can have several identical
/// listings), so both sides are sorted here by the exact comparator key before
/// diffing, which also makes the match multiset-correct (each DB row pairs with
/// exactly one incoming row with a matching key, not just a positionally lucky one).
fn listings_to_remove(
    mut db_listings: Vec<(active_listing::Model, retainer::Model)>,
    mut remove_listings: Vec<ListingView>,
) -> Vec<active_listing::Model> {
    db_listings.sort_by(|(a, ar), (b, br)| {
        remove_diff_key_model(a, &ar.name).cmp(&remove_diff_key_model(b, &br.name))
    });
    remove_listings.sort_by(|a, b| remove_diff_key_view(a).cmp(&remove_diff_key_view(b)));

    // Note: when several DB rows share an identical key (a retainer can post
    // duplicate listings), WHICH of their ids get deleted is arbitrary — the
    // websocket stream doesn't carry our row ids, so any n of the m identical
    // rows are equally correct to remove.
    PartialDiffIterator::new(
        db_listings.into_iter().map(|(l, r)| ListingData(l, r)),
        remove_listings.into_iter(),
    )
    .filter_map(|listing| match listing {
        crate::common::partial_diff_iterator::DiffItem::Same(listing, _) => Some(listing.0),
        _ => None,
    })
    .collect()
}

/// Canonical sort/merge key for `update_listings`: hq, quantity, price, retainer
/// name. Used for the incoming-view sort, the existing-db-row sort, and the merge
/// loop's comparator alike, so all three agree on what "equal" means.
fn update_diff_key_view(listing: &ListingView) -> (bool, i32, i32, &str) {
    (
        listing.hq,
        listing.quantity.unwrap_or(1) as i32,
        listing.price_per_unit.unwrap_or(listing.total) as i32,
        listing.retainer_name.as_str(),
    )
}

/// Same key, computed from a DB row + its retainer's name.
fn update_diff_key_model<'a>(
    listing: &active_listing::Model,
    retainer_name: &'a str,
) -> (bool, i32, i32, &'a str) {
    (
        listing.hq,
        listing.quantity,
        listing.price_per_unit,
        retainer_name,
    )
}

struct ListingsDiff {
    added: Vec<ListingView>,
    removed: Vec<(active_listing::Model, Option<retainer::Model>)>,
}

/// Selects the listings from an incremental `listings/add` payload that we don't
/// already hold for this item/world.
///
/// Universalis' `listings/add` is a *delta* — it carries the listings that newly
/// appeared on the board, not a snapshot of it — so this only ever yields rows to
/// insert. Matching reuses [`diff_update_listings`]' `added` side so the
/// "already have it" test is exactly the comparator the full-board path uses,
/// including its multiset handling: a retainer legitimately holding three
/// identical listings when we already store two yields exactly one insert.
///
/// Universalis' `listingID` would be an exact identity, but `active_listing` has
/// no column for it, so the key is the same (hq, quantity, price, retainer name)
/// tuple used everywhere else in this module.
fn listings_to_add(
    listings: Vec<ListingView>,
    existing_items: Vec<(active_listing::Model, Option<retainer::Model>)>,
) -> Vec<ListingView> {
    diff_update_listings(listings, existing_items).added
}

/// Diffs the incoming full listing-board view against the DB's current rows for an
/// item/world, sorting both sides by the same canonical key before merging so the
/// merge loop's comparator matches the sort exactly (previously the merge loop
/// compared fields in a different order than the sort used, causing unchanged
/// listings to be misclassified as add+remove churn).
fn diff_update_listings(
    mut listings: Vec<ListingView>,
    mut existing_items: Vec<(active_listing::Model, Option<retainer::Model>)>,
) -> ListingsDiff {
    listings.sort_by(|a, b| update_diff_key_view(a).cmp(&update_diff_key_view(b)));
    existing_items.sort_by(|(listinga, retainera), (listingb, retainerb)| {
        let retainer_name_a = retainera
            .as_ref()
            .map(|m| m.name.as_str())
            .unwrap_or_default();
        let retainer_name_b = retainerb
            .as_ref()
            .map(|m| m.name.as_str())
            .unwrap_or_default();
        update_diff_key_model(listinga, retainer_name_a)
            .cmp(&update_diff_key_model(listingb, retainer_name_b))
    });

    let mut incoming_iter = listings.into_iter();
    let mut db_iter = existing_items.into_iter();
    // compare each item, then advance the list
    let mut incoming_list = incoming_iter.next();
    let mut db_value = db_iter.next();
    let mut added = vec![];
    let mut removed = vec![];
    loop {
        match (incoming_list, db_value) {
            (Some(list), None) => {
                added.push(list);
                incoming_list = incoming_iter.next();
                db_value = None;
            }
            (None, Some(model)) => {
                removed.push(model);
                incoming_list = None;
                db_value = db_iter.next();
            }
            (Some(list), Some((model, retainer))) => {
                let retainer_name = retainer
                    .as_ref()
                    .map(|r| r.name.as_str())
                    .unwrap_or_default();
                match update_diff_key_view(&list).cmp(&update_diff_key_model(&model, retainer_name))
                {
                    std::cmp::Ordering::Less => {
                        added.push(list);
                        incoming_list = incoming_iter.next();
                        db_value = Some((model, retainer));
                    }
                    std::cmp::Ordering::Equal => {
                        // item in list, keep checking list
                        db_value = db_iter.next();
                        incoming_list = incoming_iter.next();
                    }
                    std::cmp::Ordering::Greater => {
                        removed.push((model, retainer));
                        incoming_list = Some(list);
                        db_value = db_iter.next();
                    }
                }
            }
            (None, None) => {
                // lists exhausted, exit this loop
                break;
            }
        }
    }
    ListingsDiff { added, removed }
}

impl UltrosDb {
    /// Resolves every retainer named in `listings` to a stored row, creating any
    /// we haven't seen before.
    ///
    /// Shared by the full-board and incremental ingest paths so both create
    /// retainers on exactly the same terms — a listing whose retainer we failed to
    /// store can't be inserted at all, since `create_listing` needs the id.
    async fn resolve_retainers(
        &self,
        listings: &[ListingView],
        world_id: WorldId,
    ) -> Result<HashMap<String, retainer::Model>> {
        let queried_retainers: HashSet<(String, String, i32)> = listings
            .iter()
            .map(|listing| {
                (
                    listing.retainer_name.to_string(),
                    listing.retainer_id.clone().unwrap_or_default(),
                    listing.retainer_city as i32,
                )
            })
            .collect();

        let mut retainers: HashMap<String, _> = self
            .get_retainer_ids_from_name(
                queried_retainers.iter().map(|(name, _, _)| name.as_str()),
                world_id.0,
            )
            .await?
            .into_iter()
            .map(|r| (r.name.clone(), r))
            .collect();
        // determine missing retainers
        for (name, id, retainer_city) in queried_retainers {
            if let Entry::Vacant(e) = retainers.entry(name.clone()) {
                let retainer = self
                    .store_retainer(&id, &name, world_id, retainer_city)
                    .await?;
                e.insert(retainer);
            }
        }
        Ok(retainers)
    }

    /// Applies an incremental `listings/add` event: **insert-only**.
    ///
    /// Universalis' `listings/add` carries only the listings that newly appeared
    /// on the board — measured live, the median payload is a *single* listing —
    /// not a snapshot of the board. Routing it through [`Self::update_listings`],
    /// which deletes every row absent from its input, therefore truncated each
    /// world's board down to the delta: a 19-listing board became 1, and Ultros
    /// held ~31% of the listings that were really on the market.
    ///
    /// Disappearing listings arrive separately on `listings/remove` and are
    /// handled by [`Self::remove_listings`], so nothing here needs to delete.
    /// [`Self::update_listings`] stays the path for callers that genuinely fetch a
    /// whole board (the REST catch-up service and the manual refresh route).
    #[instrument(skip(self, listings), level = "trace")]
    pub async fn add_listings(
        &self,
        listings: Vec<ListingView>,
        item_id: ItemId,
        world_id: WorldId,
    ) -> Result<Vec<(ActiveListing, Retainer)>> {
        use active_listing::*;
        let instant = Instant::now();
        let retainers = self.resolve_retainers(&listings, world_id).await?;

        let existing_items = Entity::find()
            .filter(
                Column::WorldId
                    .eq(world_id.0)
                    .and(Column::ItemId.eq(item_id.0)),
            )
            .find_also_related(retainer::Entity)
            .all(&self.db)
            .await?;

        let to_add = listings_to_add(listings, existing_items);
        let added = futures::future::join_all(to_add.iter().map(|m| {
            let retainer_id = retainers
                .get(&m.retainer_name)
                .expect("Should always have a retainer at this point.")
                .id;
            self.create_listing(m, item_id, world_id, Some(retainer_id))
        }))
        .await;

        let retainers_by_id: HashMap<i32, &retainer::Model> =
            retainers.values().map(|r| (r.id, r)).collect();
        let added: Vec<_> = added
            .into_iter()
            .flat_map(|l| {
                l.ok().map(|l| {
                    let retainer = (*retainers_by_id.get(&l.retainer_id).unwrap())
                        .clone()
                        .into();
                    (l.into(), retainer)
                })
            })
            .collect();

        // Stamp the catch-up marker even when every listing in the payload was one
        // we already held: the event still reflects a real Universalis upload at
        // this moment, so our board is as current as the stream can make it. The
        // marker is compared against their upload time, so *not* stamping would
        // make every uploaded item read as permanently behind and drag the
        // catch-up sweep into refetching the whole market.
        self.set_last_updated(world_id, item_id).await?;
        counter!("ultros_db_inserted_items", "world_id" => world_id.0.to_string())
            .increment(added.len() as u64);
        histogram!("ultros_db_add_listings_duration_seconds").record(instant.elapsed());
        Ok(added)
    }

    pub async fn remove_listings(
        &self,
        remove_listings: Vec<ListingView>,
        item_id: ItemId,
        world_id: WorldId,
    ) -> Result<Vec<(ActiveListing, Retainer)>> {
        let listings = self
            .get_all_listings_in_worlds_with_retainers(&[world_id.0], item_id)
            .await?;
        let db_listings: Vec<(active_listing::Model, retainer::Model)> = listings
            .into_iter()
            .flat_map(|(listing, retainer)| retainer.map(|r| (listing, r)))
            .collect();

        let items = try_join_all(
            listings_to_remove(db_listings, remove_listings)
                .into_iter()
                .map(|listing| async move {
                    active_listing::Entity::delete_by_id(listing.id)
                        .exec(&self.db)
                        .await
                        .map(|_| listing)
                }),
        )
        .await?;
        let retainers = items.iter().map(|i| i.retainer_id).unique();
        let retainers: HashMap<i32, Retainer> = retainer::Entity::find()
            .filter(retainer::Column::Id.is_in(retainers))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|r| (r.id, r.into()))
            .collect();
        // Only stamp the catch-up marker when we actually deleted rows. A no-op
        // remove (common: the paired full-board update already deleted them) must
        // not record "this item was ingested" — if a concurrent update_listings
        // failed, an unconditional stamp would hide the gap from catch-up forever
        // (the PR #986 bug class).
        if !items.is_empty() {
            self.set_last_updated(world_id, item_id).await?;
        }
        Ok(items
            .into_iter()
            .flat_map(|i| retainers.get(&i.retainer_id).map(|r| (i.into(), r.clone())))
            .collect())
    }

    #[instrument(skip(self))]
    pub async fn get_all_listings_in_worlds_with_retainers(
        &self,
        worlds: &[i32],
        item: ItemId,
    ) -> Result<Vec<(active_listing::Model, Option<retainer::Model>)>> {
        let instant = Instant::now();
        // OPTIMIZATION: Fetch all listings in one query
        let listings = active_listing::Entity::find()
            .filter(active_listing::Column::ItemId.eq(item.0))
            .filter(active_listing::Column::WorldId.is_in(worlds.to_vec()))
            .all(&self.db)
            .await?;

        let retainers = retainer::Entity::find()
            .filter(retainer::Column::Id.is_in(listings.iter().map(|l| l.retainer_id).unique()))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|r| (r.id, r))
            .collect::<HashMap<_, _>>();
        let data = listings
            .into_iter()
            .map(|l| {
                let retainer = retainers.get(&l.retainer_id).cloned();
                (l, retainer)
            })
            .collect();
        histogram!("ultros_db_query_listings_all_world_with_retainers_duration_seconds")
            .record(instant.elapsed());
        Ok(data)
    }

    #[instrument(skip(self))]
    pub async fn get_listings_for_items(
        &self,
        worlds: &[i32],
        items: &[i32],
    ) -> Result<HashMap<i32, ListingsWithRetainers>> {
        let instant = Instant::now();
        // OPTIMIZATION: Fetch all listings in one query
        let listings = active_listing::Entity::find()
            .filter(active_listing::Column::ItemId.is_in(items.to_vec()))
            .filter(active_listing::Column::WorldId.is_in(worlds.to_vec()))
            .all(&self.db)
            .await?;

        let retainers = retainer::Entity::find()
            .filter(retainer::Column::Id.is_in(listings.iter().map(|l| l.retainer_id).unique()))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|r| (r.id, r))
            .collect::<HashMap<_, _>>();

        let mut result: HashMap<i32, ListingsWithRetainers> = HashMap::new();

        for listing in listings {
            let retainer = retainers.get(&listing.retainer_id).cloned();
            result
                .entry(listing.item_id)
                .or_default()
                .push((listing, retainer));
        }

        histogram!("ultros_db_query_multiple_listings_all_world_with_retainers_duration_seconds")
            .record(instant.elapsed());
        Ok(result)
    }

    #[instrument(skip(self))]
    pub async fn get_all_listings_in_worlds(
        &self,
        worlds: &[i32],
        item: ItemId,
    ) -> Result<Vec<active_listing::Model>> {
        // OPTIMIZATION: Fetch all listings in one query
        let listings = active_listing::Entity::find()
            .filter(active_listing::Column::ItemId.eq(item.0))
            .filter(active_listing::Column::WorldId.is_in(worlds.iter().copied()))
            .all(&self.db)
            .await?;
        Ok(listings)
    }

    pub async fn get_listings_for_world_items(
        &self,
        world: WorldId,
        items: impl Iterator<Item = ItemId>,
    ) -> Result<Vec<active_listing::Model>> {
        use active_listing::*;
        let listings = Entity::find()
            .filter(Column::ItemId.is_in(items.map(|i| i.0)))
            .filter(Column::WorldId.eq(world.0))
            .all(&self.db)
            .await?;
        Ok(listings)
    }

    #[instrument(skip(self))]
    pub async fn get_listings_for_items_in_worlds(
        &self,
        worlds: &[i32],
        items: &[i32],
    ) -> Result<Vec<active_listing::Model>> {
        // OPTIMIZATION: Fetch all listings for all items in one query
        let listings = active_listing::Entity::find()
            .filter(active_listing::Column::ItemId.is_in(items.to_vec()))
            .filter(active_listing::Column::WorldId.is_in(worlds.to_vec()))
            .all(&self.db)
            .await?;
        Ok(listings)
    }

    #[instrument(skip(self))]
    pub async fn get_listings_for_world(
        &self,
        world: WorldId,
        item: ItemId,
    ) -> Result<Vec<active_listing::Model>> {
        use active_listing::*;
        let listings = Entity::find()
            .filter(Column::ItemId.eq(item.0))
            .filter(Column::WorldId.eq(world.0))
            .all(&self.db)
            .await?;
        Ok(listings)
    }

    #[instrument(skip(self))]
    pub async fn get_all_listings(
        &self,
        world: i32,
        item: ItemId,
    ) -> Result<Vec<active_listing::Model>> {
        use active_listing::*;
        let instant = Instant::now();
        let listings = Entity::find()
            .filter(Column::ItemId.eq(item.0))
            .filter(Column::WorldId.eq(world))
            .all(&self.db)
            .await?;

        histogram!("ultros_db_query_listings_duration_seconds").record(instant.elapsed());
        Ok(listings)
    }

    /// Updates listings assuming a pure view of the listing board
    #[instrument(skip(self, listings), level = "trace")]
    pub async fn update_listings(
        &self,
        listings: Vec<ListingView>,
        item_id: ItemId,
        world_id: WorldId,
    ) -> Result<ListingUpdate> {
        use active_listing::*;
        let instant = Instant::now();
        // Assumes that we are being given a full list of all the listings for the item and world.
        // First, query the db to see what listings it has
        // Then diff against the listings that we have (diff_update_listings sorts both sides)
        let retainers = self.resolve_retainers(&listings, world_id).await?;
        let existing_items = Entity::find()
            .filter(
                Column::WorldId
                    .eq(world_id.0)
                    .and(Column::ItemId.eq(item_id.0)),
            )
            .find_also_related(retainer::Entity)
            .all(&self.db)
            .await?;
        let ListingsDiff { added, removed } = diff_update_listings(listings, existing_items);
        let remove_iter = removed.iter();
        let added = added.iter().map(|m| {
            let retainer_id = retainers
                .get(&m.retainer_name)
                .expect("Should always have a retainer at this point.")
                .id;
            self.create_listing(m, item_id, world_id, Some(retainer_id))
        });
        let (added, _removed_result) =
            futures::future::join(futures::future::join_all(added), async move {
                let ids_to_remove: Vec<i32> = remove_iter.map(|(l, _)| l.id).collect();
                if ids_to_remove.is_empty() {
                    return Result::<usize>::Ok(0);
                }
                let res = active_listing::Entity::delete_many()
                    .filter(active_listing::Column::Id.is_in(ids_to_remove))
                    .exec(&self.db)
                    .await?;
                Result::<usize>::Ok(res.rows_affected as usize)
            })
            .await;
        let retainers_by_id: HashMap<i32, &retainer::Model> =
            retainers.values().map(|r| (r.id, r)).collect();
        let added: Vec<_> = added
            .into_iter()
            .flat_map(|l| {
                l.ok().map(|l| {
                    let retainer = (*retainers_by_id.get(&l.retainer_id).unwrap())
                        .clone()
                        .into();
                    (l.into(), retainer)
                })
            })
            .collect();
        let removed: Vec<_> = removed
            .into_iter()
            .flat_map(|(m, r)| r.map(|r| (m.into(), r.into())))
            .collect();
        self.set_last_updated(world_id, item_id).await?;
        counter!("ultros_db_inserted_items", "world_id" => world_id.0.to_string())
            .increment(added.len() as u64);
        counter!("ultros_db_removed_items", "world_id" => world_id.0.to_string())
            .increment(removed.len() as u64);
        histogram!("ultros_db_update_listings_duration_seconds").record(instant.elapsed());
        Ok((added, removed))
    }

    /// Cheapest price per (item, hq, world) for a specific set of items — the
    /// query behind the analyzer's cheapest-listing refill.
    ///
    /// This replaces `get_multiple_listings_for_worlds_hq_sensitive`, which built
    /// its result with `worlds.flat_map(|w| items.map(|i| ...))` — **one query per
    /// (world × item) pair**. Refilling 18 items across Aether's 8 worlds meant
    /// 144 round-trips, and the analyzer issued them per removed listing while
    /// holding a write lock. One grouped query answers the same question.
    ///
    /// Both qualities come back in a single call: [`ListingSummary`] carries `hq`
    /// and the analyzer keys on it, so there is no reason to split into two
    /// round-trips the way the hq-sensitive variant did.
    pub async fn cheapest_listings_for_items(
        &self,
        worlds: &[i32],
        items: &[i32],
    ) -> Result<Vec<ListingSummary>> {
        use active_listing::*;
        if worlds.is_empty() || items.is_empty() {
            return Ok(vec![]);
        }
        let instant = Instant::now();
        let summaries = Entity::find()
            .select_only()
            .column(Column::ItemId)
            .column(Column::Hq)
            .column(Column::WorldId)
            .column_as(Column::PricePerUnit.min(), "price_per_unit")
            .filter(Column::ItemId.is_in(items.to_vec()))
            .filter(Column::WorldId.is_in(worlds.to_vec()))
            .group_by(Column::ItemId)
            .group_by(Column::Hq)
            .group_by(Column::WorldId)
            .into_model::<ListingSummary>()
            .all(&self.db)
            .await?;
        histogram!("ultros_db_cheapest_listings_for_items_duration_seconds")
            .record(instant.elapsed());
        Ok(summaries)
    }

    /// The cheapest price for every (item, hq, world) currently on the market.
    ///
    /// This runs on analyzer boot and on bus-lag recovery, so it wants to be as
    /// cheap as the shape of the question allows. It used to compute
    /// `RANK() OVER (PARTITION BY item_id, hq, world_id ORDER BY price_per_unit)`
    /// over the whole table and then keep only rank 1 — Postgres cannot push that
    /// filter into the window, so it sorted every row in `active_listing` just to
    /// discard nearly all of them. `RANK()` also emits ties, so price-matched
    /// listings came back as duplicate rows.
    ///
    /// [`ListingSummary`] is a pure aggregate, so a plain `GROUP BY` answers it
    /// exactly: one row per group, no global sort, no per-row window state. With
    /// `idx_active_listing_cheapest` on (item_id, hq, world_id, price_per_unit)
    /// it can be served by an index-only scan.
    pub async fn cheapest_listings(
        &self,
    ) -> Result<impl Stream<Item = Result<ListingSummary, DbErr>> + '_, DbErr> {
        ListingSummary::find_by_statement(Statement::from_sql_and_values(
            DbBackend::Postgres,
            r#"SELECT item_id, hq, world_id, MIN(price_per_unit) AS price_per_unit
                FROM active_listing
                GROUP BY item_id, hq, world_id"#,
            vec![],
        ))
        .stream(&self.db)
        .await
    }
}

#[derive(Debug, FromQueryResult)]
pub struct ListingSummary {
    pub item_id: i32,
    pub hq: bool,
    pub price_per_unit: i32,
    pub world_id: i32,
}

#[cfg(test)]
mod diff_tests {
    //! Unit tests for the pure diff logic in `listings_to_remove` and
    //! `diff_update_listings`, run against shuffled inputs to guard against the
    //! "assumes sorted input" bug (positionally-lucky matches only) and against
    //! merge-loop/sort-key mismatches (spurious add+remove churn).

    use super::*;
    use chrono::{DateTime, Local};

    /// Minimal deterministic PRNG (xorshift32) so "shuffled" test inputs are
    /// reproducible without pulling in a `rand` dependency.
    struct Xorshift(u32);
    impl Xorshift {
        fn next_u32(&mut self) -> u32 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            self.0 = x;
            x
        }
    }

    fn shuffled<T>(mut items: Vec<T>, seed: u32) -> Vec<T> {
        let mut rng = Xorshift(seed | 1);
        let len = items.len();
        for i in (1..len).rev() {
            let j = (rng.next_u32() as usize) % (i + 1);
            items.swap(i, j);
        }
        items
    }

    fn naive_ts() -> chrono::NaiveDateTime {
        chrono::DateTime::from_timestamp(0, 0).unwrap().naive_utc()
    }

    fn local_ts() -> DateTime<Local> {
        DateTime::<Local>::from(
            chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00+00:00").unwrap(),
        )
    }

    fn listing_view(
        world_id: i32,
        retainer_name: &str,
        price: u32,
        quantity: u32,
        hq: bool,
    ) -> ListingView {
        ListingView {
            last_review_time: local_ts(),
            price_per_unit: Some(price),
            quantity: Some(quantity),
            stain_id: None,
            world_name: None,
            world_id: Some(world_id as u16),
            creator_name: None,
            creator_id: None,
            hq,
            is_crafted: false,
            listing_id: None,
            materia: vec![],
            on_mannequin: false,
            retainer_city: 1,
            retainer_id: None,
            retainer_name: retainer_name.to_string(),
            seller_id: None,
            total: price,
            tax: 0,
        }
    }

    fn db_listing(
        id: i32,
        world_id: i32,
        retainer_id: i32,
        price: i32,
        quantity: i32,
        hq: bool,
    ) -> active_listing::Model {
        active_listing::Model {
            id,
            world_id,
            item_id: 1,
            retainer_id,
            price_per_unit: price,
            quantity,
            hq,
            timestamp: naive_ts(),
        }
    }

    fn retainer_model(id: i32, world_id: i32, name: &str) -> retainer::Model {
        retainer::Model {
            id,
            world_id,
            name: name.to_string(),
            retainer_city_id: 1,
        }
    }

    /// Like `listing_view`, but lets the caller leave `price_per_unit`/`quantity`
    /// as `None` the way the real websocket feed sometimes does, to exercise the
    /// `unwrap_or` fallback semantics.
    fn listing_view_raw(
        world_id: i32,
        retainer_name: &str,
        price_per_unit: Option<u32>,
        quantity: Option<u32>,
        hq: bool,
        total: u32,
    ) -> ListingView {
        ListingView {
            last_review_time: local_ts(),
            price_per_unit,
            quantity,
            stain_id: None,
            world_name: None,
            world_id: Some(world_id as u16),
            creator_name: None,
            creator_id: None,
            hq,
            is_crafted: false,
            listing_id: None,
            materia: vec![],
            on_mannequin: false,
            retainer_city: 1,
            retainer_id: None,
            retainer_name: retainer_name.to_string(),
            seller_id: None,
            total,
            tax: 0,
        }
    }

    #[test]
    fn remove_listings_deletes_exact_shuffled_subset() {
        // Rows vary across retainer name, hq AND price (with prices repeating
        // across retainer/hq combos), so a diff that only got price ordering
        // right would still fail here.
        let world_id = 100;
        let retainers = [
            retainer_model(1, world_id, "Aaronmus"),
            retainer_model(2, world_id, "Zetamus"),
        ];
        let n = 24;
        let mut db_rows = Vec::new();
        let mut views = Vec::new();
        for i in 0..n {
            // 4 rows per price: (retainer, hq) in {A,Z} x {false,true}
            let price = 100 + (i as u32) / 4;
            let retainer = &retainers[(i as usize) % 2];
            let hq = (i / 2) % 2 == 1;
            db_rows.push((
                db_listing(i + 1, world_id, retainer.id, price as i32, 1, hq),
                retainer.clone(),
            ));
            views.push(listing_view(world_id, &retainer.name, price, 1, hq));
        }

        // pick a pseudo-random subset (every listing whose rng draw is divisible by 3) to remove
        let mut rng = Xorshift(777);
        let mut expected_removed_ids: Vec<i32> = Vec::new();
        let mut remove_views = Vec::new();
        for (i, view) in views.iter().enumerate() {
            if rng.next_u32().is_multiple_of(3) {
                expected_removed_ids.push(db_rows[i].0.id);
                remove_views.push(view.clone());
            }
        }
        expected_removed_ids.sort();
        assert!(
            !expected_removed_ids.is_empty(),
            "sanity check: subset must be non-empty"
        );

        let db_rows = shuffled(db_rows, 42);
        let remove_views = shuffled(remove_views, 99);

        let removed = listings_to_remove(db_rows, remove_views);
        let mut removed_ids: Vec<i32> = removed.iter().map(|l| l.id).collect();
        removed_ids.sort();

        assert_eq!(removed_ids, expected_removed_ids);
    }

    #[test]
    fn remove_listings_handles_duplicate_identical_listings_by_multiplicity() {
        let world_id = 200;
        let retainer = retainer_model(2, world_id, "Duplicatemus");
        // three identical listings (same price/qty/hq), distinct ids
        let db_rows = shuffled(
            vec![
                (
                    db_listing(10, world_id, retainer.id, 500, 3, true),
                    retainer.clone(),
                ),
                (
                    db_listing(11, world_id, retainer.id, 500, 3, true),
                    retainer.clone(),
                ),
                (
                    db_listing(12, world_id, retainer.id, 500, 3, true),
                    retainer.clone(),
                ),
                // an unrelated listing that should never be removed
                (
                    db_listing(13, world_id, retainer.id, 999, 1, false),
                    retainer.clone(),
                ),
            ],
            5,
        );

        // websocket says exactly two of the three identical listings are gone
        let remove_views = shuffled(
            vec![
                listing_view(world_id, &retainer.name, 500, 3, true),
                listing_view(world_id, &retainer.name, 500, 3, true),
            ],
            8,
        );

        let removed = listings_to_remove(db_rows, remove_views);
        assert_eq!(
            removed.len(),
            2,
            "exactly 2 of the 3 identical listings should be removed"
        );
        assert!(removed.iter().all(|l| [10, 11, 12].contains(&l.id)));
        assert!(!removed.iter().any(|l| l.id == 13));
    }

    #[test]
    fn update_listings_reports_no_churn_for_unchanged_listings() {
        let world_id = 300;
        let retainer_a = retainer_model(3, world_id, "Alphamus");
        let retainer_b = retainer_model(4, world_id, "Betamus");

        let existing_items = shuffled(
            vec![
                (
                    db_listing(20, world_id, retainer_a.id, 150, 2, false),
                    Some(retainer_a.clone()),
                ),
                (
                    db_listing(21, world_id, retainer_b.id, 300, 1, true),
                    Some(retainer_b.clone()),
                ),
                // duplicate row: same retainer/price/qty/hq as id 20, different id
                (
                    db_listing(22, world_id, retainer_a.id, 150, 2, false),
                    Some(retainer_a.clone()),
                ),
            ],
            11,
        );
        let listings = shuffled(
            vec![
                listing_view(world_id, &retainer_a.name, 150, 2, false),
                listing_view(world_id, &retainer_b.name, 300, 1, true),
                listing_view(world_id, &retainer_a.name, 150, 2, false),
            ],
            22,
        );

        let diff = diff_update_listings(listings, existing_items);
        assert!(
            diff.added.is_empty(),
            "no listings should be added, got {} added",
            diff.added.len()
        );
        assert!(
            diff.removed.is_empty(),
            "no listings should be removed, got ids {:?}",
            diff.removed.iter().map(|(m, _)| m.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn update_listings_unwraps_missing_quantity_consistently_between_sort_and_merge() {
        // Two unchanged listings whose *raw* `Option<u32>` quantity ordering
        // disagrees with their *resolved* (`unwrap_or(1)`) ordering: raw `None`
        // sorts before `Some(0)`, but the resolved quantity treats `None` as `1`,
        // which is greater than `0`. A sort key and merge comparator that don't
        // agree on this unwrap semantic put the two sides out of step and
        // misclassify both rows as a remove+add pair even though nothing changed.
        let world_id = 300;
        let retainer = retainer_model(3, world_id, "Alphamus");

        let low_qty = db_listing(20, world_id, retainer.id, 500, 0, false);
        let default_qty = db_listing(21, world_id, retainer.id, 500, 1, false);

        let existing_items = shuffled(
            vec![
                (low_qty.clone(), Some(retainer.clone())),
                (default_qty.clone(), Some(retainer.clone())),
            ],
            11,
        );
        let listings = shuffled(
            vec![
                listing_view_raw(world_id, &retainer.name, Some(500), Some(0), false, 500),
                listing_view_raw(world_id, &retainer.name, Some(500), None, false, 500),
            ],
            22,
        );

        let diff = diff_update_listings(listings, existing_items);
        assert!(
            diff.added.is_empty(),
            "no listings should be added, got {} added",
            diff.added.len()
        );
        assert!(
            diff.removed.is_empty(),
            "no listings should be removed, got ids {:?}",
            diff.removed.iter().map(|(m, _)| m.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn update_listings_adds_and_removes_only_the_changed_rows() {
        let world_id = 400;
        let retainer_a = retainer_model(5, world_id, "Gammamus");

        // db has 3 unchanged rows plus one that will disappear from the board
        let unchanged: Vec<_> = (0..3)
            .map(|i| db_listing(30 + i, world_id, retainer_a.id, 200 + i, 5, false))
            .collect();
        let stale = db_listing(99, world_id, retainer_a.id, 999, 9, true);

        let existing_items = shuffled(
            unchanged
                .iter()
                .cloned()
                .map(|m| (m, Some(retainer_a.clone())))
                .chain(std::iter::once((stale.clone(), Some(retainer_a.clone()))))
                .collect(),
            3,
        );

        // incoming view: same unchanged rows, plus one brand-new listing, minus the stale one
        let mut listings: Vec<_> = unchanged
            .iter()
            .map(|m| {
                listing_view(
                    world_id,
                    &retainer_a.name,
                    m.price_per_unit as u32,
                    m.quantity as u32,
                    m.hq,
                )
            })
            .collect();
        let new_listing = listing_view(world_id, &retainer_a.name, 12345, 1, true);
        listings.push(new_listing.clone());
        let listings = shuffled(listings, 4);

        let diff = diff_update_listings(listings, existing_items);

        assert_eq!(
            diff.added.len(),
            1,
            "exactly one new listing should be added"
        );
        assert_eq!(diff.added[0].price_per_unit, new_listing.price_per_unit);

        assert_eq!(
            diff.removed.len(),
            1,
            "exactly the stale listing should be removed"
        );
        assert_eq!(diff.removed[0].0.id, stale.id);
    }

    #[test]
    fn update_listings_keeps_hq_row_when_nq_row_disappears() {
        // Regression for the sort/merge key mismatch (Bug 2): the old code sorted
        // both sides by (hq, quantity, price, name) but merged by (price,
        // quantity, name, hq). With an NQ row at a higher price and an HQ row at
        // a lower price, the merge compared the HQ view (price 5) against the
        // first-sorted NQ model (price 10), saw Less, and pushed the HQ view as
        // "added"; the exhausted-incoming arm then swept BOTH db rows into
        // "removed" — pointless delete+reinsert churn for the unchanged HQ row.
        // The fixed code must remove only the NQ row and add nothing.
        let world_id = 500;
        let retainer = retainer_model(6, world_id, "Deltamus");
        let nq = db_listing(40, world_id, retainer.id, 10, 1, false);
        let hq = db_listing(41, world_id, retainer.id, 5, 1, true);
        let existing_items = vec![
            (nq.clone(), Some(retainer.clone())),
            (hq.clone(), Some(retainer.clone())),
        ];
        // incoming board only has the HQ listing
        let listings = vec![listing_view(world_id, &retainer.name, 5, 1, true)];

        let diff = diff_update_listings(listings, existing_items);
        assert!(
            diff.added.is_empty(),
            "HQ listing is unchanged; nothing should be added, got {} added",
            diff.added.len()
        );
        assert_eq!(
            diff.removed.len(),
            1,
            "only the NQ listing should be removed, got ids {:?}",
            diff.removed.iter().map(|(m, _)| m.id).collect::<Vec<_>>()
        );
        assert_eq!(diff.removed[0].0.id, nq.id);
    }

    /// The bug this module's `add_listings` path exists to fix: a `listings/add`
    /// delta carrying one listing, applied to a board of 19, used to go through
    /// `diff_update_listings` — which reports the other 18 as `removed` because it
    /// assumes its input is a whole board. `update_listings` then deleted them.
    ///
    /// This asserts both halves: the old full-board diff really does condemn the
    /// other 18, and `listings_to_add` returns only the single genuinely-new
    /// listing — it has no way to express a removal at all.
    #[test]
    fn add_delta_inserts_only_the_new_listing_and_never_removes_the_board() {
        let world_id = 54;
        let retainer = retainer_model(1, world_id, "Marywake");
        // 18 listings already on the board, priced above the newcomer
        let board: Vec<_> = (0..18)
            .map(|i| {
                (
                    db_listing(100 + i, world_id, retainer.id, 380_000 + i, 1, false),
                    Some(retainer.clone()),
                )
            })
            .collect();
        // the websocket delta: one brand-new undercut listing
        let delta = vec![listing_view(world_id, &retainer.name, 379_996, 1, false)];

        // The old path condemns the whole rest of the board.
        let old = diff_update_listings(delta.clone(), board.clone());
        assert_eq!(
            old.removed.len(),
            18,
            "regression guard: the full-board diff is exactly what truncated the board"
        );

        // The new path can only ever insert.
        let to_add = listings_to_add(delta, board);
        assert_eq!(to_add.len(), 1, "only the new listing should be inserted");
        assert_eq!(to_add[0].price_per_unit, Some(379_996));
    }

    #[test]
    fn add_delta_does_not_duplicate_a_listing_we_already_hold() {
        let world_id = 54;
        let retainer = retainer_model(1, world_id, "Marywake");
        let existing = shuffled(
            vec![
                (
                    db_listing(1, world_id, retainer.id, 379_996, 1, false),
                    Some(retainer.clone()),
                ),
                (
                    db_listing(2, world_id, retainer.id, 400_000, 2, true),
                    Some(retainer.clone()),
                ),
            ],
            13,
        );
        // Universalis re-sends a listing we already stored.
        let delta = vec![listing_view(world_id, &retainer.name, 379_996, 1, false)];

        assert!(
            listings_to_add(delta, existing).is_empty(),
            "a re-sent listing must not be inserted twice"
        );
    }

    #[test]
    fn add_delta_respects_multiplicity_of_identical_listings() {
        // A retainer may legitimately hold several identical listings. If we
        // already store two and the delta says there are three, exactly one is new
        // — matching by key alone (rather than multiset) would insert none.
        let world_id = 54;
        let retainer = retainer_model(1, world_id, "Duplicatemus");
        let existing = vec![
            (
                db_listing(1, world_id, retainer.id, 500, 3, true),
                Some(retainer.clone()),
            ),
            (
                db_listing(2, world_id, retainer.id, 500, 3, true),
                Some(retainer.clone()),
            ),
        ];
        let delta = vec![
            listing_view(world_id, &retainer.name, 500, 3, true),
            listing_view(world_id, &retainer.name, 500, 3, true),
            listing_view(world_id, &retainer.name, 500, 3, true),
        ];

        let to_add = listings_to_add(delta, existing);
        assert_eq!(to_add.len(), 1, "exactly one of the three is new");
    }

    #[test]
    fn add_delta_into_an_empty_board_inserts_everything() {
        let world_id = 54;
        let retainer = retainer_model(1, world_id, "Firstmus");
        let delta = shuffled(
            vec![
                listing_view(world_id, &retainer.name, 100, 1, false),
                listing_view(world_id, &retainer.name, 200, 1, true),
                listing_view(world_id, &retainer.name, 300, 2, false),
            ],
            7,
        );
        assert_eq!(listings_to_add(delta, vec![]).len(), 3);
    }

    /// Universalis' websocket sends `worldID: null` on every listing — the world
    /// is carried once on the event envelope, not per listing. Measured live:
    /// 570/570 `listings/remove` and 773/773 `listings/add` payload entries had a
    /// null `worldID`.
    ///
    /// The remove key used to lead with `world_id.unwrap_or_default()`, so the
    /// incoming side was always world `0` while the DB side held the real world.
    /// The tuples could never compare `Equal`, `PartialDiffIterator` never yielded
    /// `Same`, and `remove_listings` deleted nothing at all — retainers that
    /// repriced accumulated a row per price forever.
    ///
    /// Every other test in this module builds views with `world_id: Some(..)`,
    /// which production never does, so they all passed straight through the bug.
    /// This one uses `None` on purpose.
    #[test]
    fn remove_listings_matches_views_with_no_world_id_like_the_websocket_sends() {
        let world_id = 63;
        let retainer = retainer_model(1, world_id, "Luicy");
        // A retainer repriced this item: the old row should go, the new one stay.
        let old_price = db_listing(1, world_id, retainer.id, 24_999, 1, false);
        let new_price = db_listing(2, world_id, retainer.id, 96_998, 1, false);
        let db_rows = vec![
            (old_price.clone(), retainer.clone()),
            (new_price.clone(), retainer.clone()),
        ];

        // The websocket's remove payload, exactly as it arrives: no world id.
        let mut view = listing_view(world_id, &retainer.name, 24_999, 1, false);
        view.world_id = None;

        let removed = listings_to_remove(db_rows, vec![view]);
        assert_eq!(
            removed.len(),
            1,
            "a remove view with no world id must still match the stored row"
        );
        assert_eq!(removed[0].id, old_price.id);
    }

    /// The same reprice, but with several items in flight, to confirm the key
    /// still discriminates once world is no longer part of it.
    #[test]
    fn remove_listings_without_world_id_still_discriminates_between_listings() {
        let world_id = 63;
        let luicy = retainer_model(1, world_id, "Luicy");
        let other = retainer_model(2, world_id, "Someoneelse");
        let target = db_listing(1, world_id, luicy.id, 147_901, 1, false);
        let db_rows = vec![
            (target.clone(), luicy.clone()),
            // same price, different retainer
            (db_listing(2, world_id, other.id, 147_901, 1, false), other),
            // same retainer, different price
            (
                db_listing(3, world_id, luicy.id, 147_879, 1, false),
                luicy.clone(),
            ),
            // same retainer and price, but HQ
            (
                db_listing(4, world_id, luicy.id, 147_901, 1, true),
                luicy.clone(),
            ),
        ];

        let mut view = listing_view(world_id, &luicy.name, 147_901, 1, false);
        view.world_id = None;

        let removed = listings_to_remove(db_rows, vec![view]);
        assert_eq!(removed.len(), 1, "exactly one row matches");
        assert_eq!(removed[0].id, target.id);
    }

    #[test]
    fn remove_listings_matches_rows_stored_from_none_price_views() {
        // A listing that arrived with `price_per_unit: None` was stored by
        // `create_listing` with price = total and quantity = 1. When the
        // websocket later removes it (again with None price/quantity), the
        // remove key must resolve those Nones the same way, or the row can never
        // match and lingers as a permanent phantom. The old key used
        // `unwrap_or_default()` (= 0), which never matched the stored
        // (price = total, quantity = 1) row.
        let world_id = 600;
        let retainer = retainer_model(7, world_id, "Phantomus");
        // stored with the insert-path fallbacks: price = total (750), qty = 1
        let stored = db_listing(50, world_id, retainer.id, 750, 1, false);
        let keeper = db_listing(51, world_id, retainer.id, 200, 2, false);
        let db_rows = vec![
            (stored.clone(), retainer.clone()),
            (keeper.clone(), retainer.clone()),
        ];
        let remove_views = vec![listing_view_raw(
            world_id,
            &retainer.name,
            None,
            None,
            false,
            750,
        )];

        let removed = listings_to_remove(db_rows, remove_views);
        assert_eq!(
            removed.len(),
            1,
            "the None-price listing must be matched for removal"
        );
        assert_eq!(removed[0].id, stored.id);
        // MAJOR-1 note: `set_last_updated` gating lives in the async
        // `remove_listings` DB method and can't be unit-tested here; the pure
        // contract this test relies on is that an empty return from
        // `listings_to_remove` means nothing was deleted (and thus no marker
        // stamp).
    }
}
