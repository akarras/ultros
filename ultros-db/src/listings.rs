use anyhow::Result;
use futures::{Stream, future::try_join_all};
use itertools::Itertools;
use metrics::{counter, histogram};
use migration::DbErr;
use sea_orm::{
    ColumnTrait, DbBackend, EntityTrait, ExprTrait, FromQueryResult, QueryFilter, Statement,
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

impl PartialEq<ListingView> for ListingData {
    fn eq(&self, other: &ListingView) -> bool {
        self.0.world_id == other.world_id.unwrap_or_default() as i32
            && self.0.price_per_unit == other.price_per_unit.unwrap_or_default() as i32
            && self.0.quantity == other.quantity.unwrap_or_default() as i32
            && self.0.hq == other.hq
            && self.1.name == other.retainer_name
        // timestamp intentionally ignored
    }
}

pub type ListingUpdate = (
    Vec<(ActiveListing, Retainer)>,
    Vec<(ActiveListing, Retainer)>,
);

pub type ListingsWithRetainers = Vec<(active_listing::Model, Option<retainer::Model>)>;

struct ListingData(active_listing::Model, retainer::Model);

impl PartialOrd<ListingView> for ListingData {
    fn partial_cmp(&self, other: &ListingView) -> Option<std::cmp::Ordering> {
        let ListingData(listing, retainer) = self;
        match (listing.world_id as u16).partial_cmp(&other.world_id.unwrap_or_default()) {
            Some(core::cmp::Ordering::Equal) => {}
            ord => return ord,
        }
        match retainer.name.partial_cmp(&other.retainer_name) {
            Some(core::cmp::Ordering::Equal) => {}
            ord => return ord,
        }
        match listing
            .price_per_unit
            .partial_cmp(&(other.price_per_unit.unwrap_or_default() as i32))
        {
            Some(core::cmp::Ordering::Equal) => {}
            ord => return ord,
        }
        match listing
            .quantity
            .partial_cmp(&(other.quantity.unwrap_or_default() as i32))
        {
            Some(core::cmp::Ordering::Equal) => {}
            ord => return ord,
        }
        listing.hq.partial_cmp(&other.hq)
    }
}

/// Sort key for `active_listing::Model`/`retainer::Model` pairs matching the field
/// order of `ListingData`'s `PartialOrd<ListingView>` impl above: world, retainer
/// name, price, quantity, hq.
fn remove_diff_key_model<'a>(
    listing: &active_listing::Model,
    retainer_name: &'a str,
) -> (u16, &'a str, i32, i32, bool) {
    (
        listing.world_id as u16,
        retainer_name,
        listing.price_per_unit,
        listing.quantity,
        listing.hq,
    )
}

/// Same key, computed from the incoming websocket view, using the same
/// `unwrap_or_default` semantics as the `PartialOrd<ListingView>` impl.
fn remove_diff_key_view(listing: &ListingView) -> (u16, &str, i32, i32, bool) {
    (
        listing.world_id.unwrap_or_default(),
        listing.retainer_name.as_str(),
        listing.price_per_unit.unwrap_or_default() as i32,
        listing.quantity.unwrap_or_default() as i32,
        listing.hq,
    )
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
    db_listings: Vec<(active_listing::Model, retainer::Model)>,
    mut remove_listings: Vec<ListingView>,
) -> Vec<active_listing::Model> {
    let mut db_listings = db_listings;
    db_listings.sort_by(|(a, ar), (b, br)| {
        remove_diff_key_model(a, &ar.name).cmp(&remove_diff_key_model(b, &br.name))
    });
    remove_listings.sort_by(|a, b| remove_diff_key_view(a).cmp(&remove_diff_key_view(b)));

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
        self.set_last_updated(world_id, item_id).await?;
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

    pub async fn cheapest_listings(
        &self,
    ) -> Result<impl Stream<Item = Result<ListingSummary, DbErr>> + '_, DbErr> {
        ListingSummary::find_by_statement(Statement::from_sql_and_values(
            DbBackend::Postgres,
            r#"SELECT ranks.* FROM (SELECT l.item_id, l.hq, l.price_per_unit, l.world_id,
                RANK() OVER (PARTITION BY l.item_id, l.hq, l.world_id ORDER BY l.price_per_unit ASC) listing_rank
                FROM active_listing l) ranks
                WHERE ranks.listing_rank = 1"#,
            vec![],
        )).stream(&self.db).await
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
        let world_id = 100;
        let retainer = retainer_model(1, world_id, "Aaronmus");
        let n = 25;
        let mut db_rows = Vec::new();
        let mut views = Vec::new();
        for i in 0..n {
            let price = 100 + i as u32;
            db_rows.push((
                db_listing(i + 1, world_id, retainer.id, price as i32, 1, false),
                retainer.clone(),
            ));
            views.push(listing_view(world_id, &retainer.name, price, 1, false));
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
}
