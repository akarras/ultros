use std::{
    collections::{HashMap, HashSet},
    time::Instant,
};

use crate::{
    UltrosDb,
    common_type_conversions::SaleHistoryReturn,
    entity::{sale_history, unknown_final_fantasy_character},
};
use anyhow::Result;
use chrono::{Duration, NaiveDateTime, Utc};

use futures::{Stream, future::try_join_all};
use itertools::Itertools;
use metrics::histogram;
use migration::{
    DbErr,
    sea_orm::{ColumnTrait, EntityTrait, QueryFilter, Set},
};
use sea_orm::{
    ActiveModelTrait, ActiveValue, DbBackend, FromQueryResult, QueryOrder, QuerySelect, Statement,
};
use tracing::{instrument, warn};
use ultros_api_types::{SaleHistory, UnknownCharacter};
use universalis::{ItemId, SaleView, WorldId};

impl UltrosDb {
    /// Stores a sale from a given sale view.
    /// Demands that a world name for the sale is provided as it is optional on the sale view, but can be determined other ways
    #[instrument(skip(self, sales))]
    pub async fn update_sales(
        &self,
        sales: Vec<SaleView>,
        item_id: ItemId,
        world_id: WorldId,
    ) -> Result<Vec<(SaleHistory, UnknownCharacter)>> {
        let instant = Instant::now();
        let recorded_sales = self
            .insert_unrecorded_sales(sales, item_id, world_id)
            .await?;
        // Only claim the item as ingested once the sales have actually landed.
        // `listing_last_updated` is the marker the catch-up service diffs against
        // Universalis' recently-updated feed, so bumping it ahead of a write that
        // then fails makes the resulting gap invisible to every later catch-up
        // pass -- the item looks fresher than their upload and is never retried.
        self.set_last_updated(world_id, item_id).await?;
        histogram!("ultrso_db_update_sales_duration_seconds").record(instant.elapsed());
        Ok(recorded_sales)
    }

    async fn insert_unrecorded_sales(
        &self,
        mut sales: Vec<SaleView>,
        item_id: ItemId,
        world_id: WorldId,
    ) -> Result<Vec<(SaleHistory, UnknownCharacter)>> {
        use sale_history::*;
        // check if the sales have already been logged
        if sales.is_empty() {
            return Ok(vec![]);
        }

        // check for any sales that have already been posted
        let limit = sales.len() as u64;
        let already_recorded_sales = self
            .get_sale_history_for_item(world_id.0, item_id.0, limit)
            .await?;
        let buyers = self.lookup_buyer_names(&sales).await?;
        sales.retain(|sale| {
            let buyer = buyers
                .get(&sale.buyer_name)
                .expect("Should always have gotten a buyer model");
            !already_recorded_sales.iter().any(|recorded| {
                sale.hq == recorded.hq
                    && buyer.id == recorded.buying_character_id
                    && sale.quantity == recorded.quantity
                    && sale.timestamp.timestamp() == recorded.sold_date.and_utc().timestamp()
            })
        });
        if sales.is_empty() {
            return Ok(vec![]);
        }
        // Insert with RETURNING so the rows we hand back carry their real
        // Postgres ids. The ClickHouse dual-write uses `sale_history.id` as the
        // discriminator in the `sales` ORDER BY key, so returning `id: 0` here
        // would (a) collapse distinct same-second sales of the same
        // item/hq/world into one ClickHouse row and (b) make the backfill --
        // which reads real ids straight out of Postgres -- write a *second*,
        // never-merging copy of every sale the live path already wrote.
        let inserted = Entity::insert_many(sales.into_iter().map(|sale| {
            let buyer = buyers
                .get(&sale.buyer_name)
                .expect("Should always have a buyer model");
            let SaleView {
                hq,
                price_per_unit,
                quantity,
                ..
            } = sale;
            ActiveModel {
                id: Default::default(),
                quantity: Set(quantity),
                price_per_item: Set(price_per_unit),
                buying_character_id: Set(buyer.id),
                hq: Set(hq),
                sold_item_id: Set(item_id.0),
                sold_date: Set(sale.timestamp.naive_utc()),
                world_id: Set(world_id.0),
            }
        }))
        .exec_with_returning(&self.db)
        .await?;
        let buyers_by_id: HashMap<i32, _> = buyers.into_values().map(|b| (b.id, b)).collect();
        Ok(attach_buyers(inserted, &buyers_by_id))
    }

    pub async fn get_sale_history_from_multiple_worlds(
        &self,
        world_ids: impl Iterator<Item = i32>,
        item_id: i32,
        limit: u64,
    ) -> Result<Vec<SaleHistoryReturn>, anyhow::Error> {
        let all = futures::future::try_join_all(
            world_ids.map(|world_id| self.get_sale_history_for_item(world_id, item_id, limit)),
        )
        .await;

        let mut sales: Vec<_> = all?.into_iter().flat_map(|w| w.into_iter()).collect();

        // ⚡ Bolt: Optimization: Extract top N elements in O(N) time with select_nth_unstable_by_key before sorting
        let limit_usize = limit as usize;
        if sales.len() > limit_usize {
            sales.select_nth_unstable_by_key(limit_usize, |sale| std::cmp::Reverse(sale.sold_date));
            sales.truncate(limit_usize);
        }
        sales.sort_unstable_by_key(|sale| std::cmp::Reverse(sale.sold_date));

        let buyers = unknown_final_fantasy_character::Entity::find()
            .filter(
                unknown_final_fantasy_character::Column::Id
                    .is_in(sales.iter().map(|s| s.buying_character_id).unique()),
            )
            .all(&self.db)
            .await?
            .into_iter()
            .map(|c| (c.id, c))
            .collect::<HashMap<_, _>>();
        let sales = sales
            .into_iter()
            .map(|sale| {
                let buyer = buyers.get(&sale.buying_character_id).cloned();
                SaleHistoryReturn(sale, buyer)
            })
            .collect();
        Ok(sales)
    }

    async fn lookup_buyer_names(
        &self,
        sales: &[SaleView],
    ) -> Result<HashMap<String, unknown_final_fantasy_character::Model>, anyhow::Error> {
        // get all the unique buyer names
        let buyers: HashSet<_> = sales.iter().map(|b| &b.buyer_name).collect();
        Ok(try_join_all(buyers.into_iter().map(|name| async move {
            let buyer = unknown_final_fantasy_character::Entity::find()
                .filter(unknown_final_fantasy_character::Column::Name.eq(name))
                .one(&self.db)
                .await?;
            let buyer = match buyer {
                Some(buyer) => buyer,
                None => {
                    let result = unknown_final_fantasy_character::ActiveModel {
                        name: ActiveValue::Set(name.to_string()),
                        ..Default::default()
                    }
                    .insert(&self.db)
                    .await;
                    match result {
                        Ok(m) => m,
                        // the most common error here is a duplicate key, in this case we can just look them up now.
                        Err(e) => unknown_final_fantasy_character::Entity::find()
                            .filter(unknown_final_fantasy_character::Column::Name.eq(name))
                            .one(&self.db)
                            .await?
                            .ok_or(e)?,
                    }
                }
            };
            Ok::<_, anyhow::Error>((buyer.name.clone(), buyer))
        }))
        .await?
        .into_iter()
        .collect())
    }

    pub async fn get_sale_history_for_item(
        &self,
        world_id: i32,
        item_id: i32,
        limit: u64,
    ) -> Result<Vec<sale_history::Model>, anyhow::Error> {
        let start = Instant::now();
        let data = sale_history::Entity::find()
            .filter(sale_history::Column::SoldItemId.eq(item_id))
            .filter(sale_history::Column::WorldId.eq(world_id))
            .order_by_desc(sale_history::Column::SoldDate)
            .limit(limit)
            .all(&self.db)
            .await?;
        histogram!("ultros_db_query_sale_history_duration_seconds").record(start.elapsed());
        Ok(data)
    }

    /// Lean projection of recent sale history for charting. Returns up to `limit` rows
    /// across all `world_ids`, sorted newest-first. Skips the buyer-name join that
    /// `get_sale_history_from_multiple_worlds` performs, which is the dominant cost
    /// when callers don't need names (e.g. the chart).
    pub async fn get_compact_sale_history(
        &self,
        world_ids: impl Iterator<Item = i32>,
        item_id: i32,
        limit: u64,
    ) -> Result<Vec<sale_history::Model>, anyhow::Error> {
        let per_world = futures::future::try_join_all(
            world_ids.map(|world_id| self.get_sale_history_for_item(world_id, item_id, limit)),
        )
        .await?;
        let mut sales: Vec<sale_history::Model> = per_world.into_iter().flatten().collect();

        // ⚡ Bolt: Optimization: Extract top N elements in O(N) time with select_nth_unstable_by_key before sorting
        let limit_usize = limit as usize;
        if limit_usize > 0 && sales.len() > limit_usize {
            sales.select_nth_unstable_by_key(limit_usize, |s| std::cmp::Reverse(s.sold_date));
            sales.truncate(limit_usize);
        }
        sales.sort_unstable_by_key(|s| std::cmp::Reverse(s.sold_date));
        Ok(sales)
    }

    pub async fn last_n_sales(
        &self,
        n_sales: i32,
    ) -> Result<impl Stream<Item = Result<AbbreviatedSaleData, DbErr>> + '_, DbErr> {
        AbbreviatedSaleData::find_by_statement(Statement::from_sql_and_values(
                DbBackend::Postgres,
                r#"SELECT filter.* FROM (SELECT h.sold_item_id, h.hq, h.price_per_item, h.sold_date, h.world_id,
                RANK() OVER (PARTITION BY h.sold_item_id, h.hq, h.world_id ORDER BY h.sold_date DESC) sale_rank
                FROM sale_history h) filter
                WHERE filter.sale_rank <= $1
                "#,
                vec![n_sales.into()],
            ))
            .stream(&self.db)
            .await
    }

    #[instrument(skip(self))]
    pub async fn stream_sales_within_days(
        &self,
        days: i64,
        world_id: i32,
    ) -> Result<impl Stream<Item = Result<sale_history::Model, DbErr>> + '_, anyhow::Error> {
        Ok(sale_history::Entity::find()
            .filter(sale_history::Column::WorldId.eq(world_id))
            .filter(sale_history::Column::SoldDate.gt(Utc::now() - Duration::days(days)))
            .stream(&self.db)
            .await?)
    }

    /// Stream sales for a single (world, half-open date range). Used by the
    /// ClickHouse backfill, which chunks history into `(world_id, year-month)`
    /// units for resumability.
    #[instrument(skip(self))]
    pub async fn stream_sales_in_range(
        &self,
        world_id: i32,
        start: NaiveDateTime,
        end: NaiveDateTime,
    ) -> Result<impl Stream<Item = Result<sale_history::Model, DbErr>> + '_, anyhow::Error> {
        Ok(sale_history::Entity::find()
            .filter(sale_history::Column::WorldId.eq(world_id))
            .filter(sale_history::Column::SoldDate.gte(start))
            .filter(sale_history::Column::SoldDate.lt(end))
            .stream(&self.db)
            .await?)
    }
}

/// Pair freshly inserted `sale_history` rows back up with their buyers to form
/// the event payload.
///
/// The rows come straight out of `INSERT ... RETURNING`, so `model.id` is the
/// real Postgres id — that id is what the ClickHouse dual-write puts in
/// `SaleRow::pg_id`, and it is the discriminator in the `sales` ORDER BY key.
/// Anything that loses it here silently corrupts ClickHouse dedup.
///
/// Postgres doesn't promise RETURNING row order matches the VALUES order, so
/// buyers are re-attached by id rather than zipped positionally.
fn attach_buyers(
    inserted: Vec<sale_history::Model>,
    buyers_by_id: &HashMap<i32, unknown_final_fantasy_character::Model>,
) -> Vec<(SaleHistory, UnknownCharacter)> {
    inserted
        .into_iter()
        .filter_map(|model| {
            let Some(buyer) = buyers_by_id.get(&model.buying_character_id) else {
                // Unreachable: every id here came from a buyer we just looked
                // up or created. Skip rather than panic on the ingest path.
                warn!(
                    buying_character_id = model.buying_character_id,
                    "recorded sale references an unknown buyer; dropping from event payload"
                );
                return None;
            };
            let record: SaleHistory = SaleHistoryReturn(model, Some(buyer.clone())).into();
            Some((record, buyer.into()))
        })
        .collect()
}

#[derive(Debug, FromQueryResult)]
pub struct AbbreviatedSaleData {
    pub sold_item_id: i32,
    pub hq: bool,
    pub price_per_item: i32,
    pub sold_date: NaiveDateTime,
    pub world_id: i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn buyer(id: i32, name: &str) -> unknown_final_fantasy_character::Model {
        unknown_final_fantasy_character::Model {
            id,
            name: name.to_string(),
        }
    }

    fn inserted_row(id: i32, buyer_id: i32, second: u32) -> sale_history::Model {
        sale_history::Model {
            id,
            quantity: 1,
            price_per_item: 1000,
            buying_character_id: buyer_id,
            hq: false,
            sold_item_id: 5,
            sold_date: NaiveDate::from_ymd_opt(2026, 5, 15)
                .unwrap()
                .and_hms_opt(12, 0, second)
                .unwrap(),
            world_id: 40,
        }
    }

    #[test]
    fn attach_buyers_preserves_the_postgres_ids() {
        // Regression guard: this used to hand back `id: 0` for every sale,
        // which made the ClickHouse `sales` ORDER BY key non-unique.
        let buyers = HashMap::from([(7, buyer(7, "Buyer One"))]);
        let rows = vec![inserted_row(101, 7, 0), inserted_row(102, 7, 1)];

        let attached = attach_buyers(rows, &buyers);

        let ids: Vec<i32> = attached.iter().map(|(sale, _)| sale.id).collect();
        assert_eq!(ids, vec![101, 102]);
        assert!(ids.iter().all(|id| *id != 0));
    }

    #[test]
    fn attach_buyers_keeps_same_second_sales_distinct() {
        // Two sales of the same item/hq/world in the same second: `id` is the
        // only thing telling them apart, both in PG and in the CH sort key.
        let buyers = HashMap::from([(7, buyer(7, "Buyer One"))]);
        let rows = vec![inserted_row(101, 7, 30), inserted_row(102, 7, 30)];

        let attached = attach_buyers(rows, &buyers);

        assert_eq!(attached.len(), 2);
        assert_ne!(attached[0].0.id, attached[1].0.id);
        assert_eq!(attached[0].0.sold_date, attached[1].0.sold_date);
    }

    #[test]
    fn attach_buyers_matches_by_id_not_position() {
        // RETURNING order isn't guaranteed to match the VALUES order.
        let buyers = HashMap::from([(7, buyer(7, "Buyer One")), (9, buyer(9, "Buyer Two"))]);
        let rows = vec![inserted_row(101, 9, 0), inserted_row(102, 7, 1)];

        let attached = attach_buyers(rows, &buyers);

        assert_eq!(attached[0].0.buyer_name.as_deref(), Some("Buyer Two"));
        assert_eq!(attached[1].0.buyer_name.as_deref(), Some("Buyer One"));
        assert_eq!(attached[0].1.name, "Buyer Two");
        assert_eq!(attached[1].1.name, "Buyer One");
    }

    #[test]
    fn attach_buyers_drops_rows_with_no_known_buyer() {
        let buyers = HashMap::from([(7, buyer(7, "Buyer One"))]);
        let rows = vec![inserted_row(101, 7, 0), inserted_row(102, 999, 1)];

        let attached = attach_buyers(rows, &buyers);

        assert_eq!(attached.len(), 1);
        assert_eq!(attached[0].0.id, 101);
    }
}
