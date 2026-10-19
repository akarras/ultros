//! Read-only price assistance for an account's registered retainers. Private
//! captures stay private; listing and price changes are made manually in game.
use super::{PluginError, PluginPrincipal, Result, auth::authorize, query};
use crate::UltrosDb;
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use sea_orm::TransactionTrait;
use std::collections::HashMap;
use ultros_api_types::plugin::{PrivateListing, RetainerPriceLookup, RetainerPriceQuote};

fn suggestion(
    competitor: Option<i64>,
    checked: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> (Option<i64>, &'static str) {
    let Some(price @ 1..=999_999_999) = competitor else {
        return (None, "no_competitors");
    };
    let Some(checked) = checked else {
        return (None, "unknown_age");
    };
    if checked < now - Duration::minutes(30) || checked > now + Duration::minutes(1) {
        return (None, "stale");
    }
    (Some((price - 1).max(1)), "fresh")
}

impl UltrosDb {
    pub async fn plugin_retainer_prices(
        &self,
        principal: &PluginPrincipal,
        retainer_id: i64,
    ) -> Result<RetainerPriceLookup> {
        let txn = self.db.begin().await?;
        authorize(&txn, principal, Some("retainers:read")).await?;
        // Recheck both private registration and the normal website assignment.
        // A stale/missing link is a resource error, not a credential failure.
        let row = query(&txn,
            "SELECT p.name,r.id AS public_id,r.world_id,w.name AS world_name
             FROM plugin_retainer p
             JOIN plugin_character c ON c.id=p.registered_character AND c.owner=p.owner AND c.registered
             JOIN owned_retainers o ON o.retainer_id=p.public_retainer_id AND o.discord_id=p.owner
             JOIN retainer r ON r.id=p.public_retainer_id AND r.world_id=c.home_world AND r.name=p.name
             JOIN world w ON w.id=r.world_id
             WHERE p.owner=$1 AND p.id=$2 FOR SHARE OF p,c,o,r",
            vec![principal.owner.into(), retainer_id.into()])
            .await?.into_iter().next().ok_or(PluginError::Conflict("retainer_registration_required"))?;
        let world: i32 = row.try_get("", "world_id")?;
        let snapshot = query(&txn,
            "SELECT observed_at,listings FROM plugin_snapshot WHERE owner=$1 AND retainer_id=$2 ORDER BY revision DESC LIMIT 1",
            vec![principal.owner.into(),retainer_id.into()]).await?;
        let (listings, observed_at, source) = if let Some(snapshot) = snapshot.first() {
            let listings: Vec<PrivateListing> =
                serde_json::from_value(snapshot.try_get("", "listings")?)
                    .map_err(|_| PluginError::Storage)?;
            if listings.len() > 20 {
                return Err(PluginError::Storage);
            }
            (
                listings,
                Some(snapshot.try_get("", "observed_at")?),
                "in_game",
            )
        } else {
            let public = query(
                &txn,
                "SELECT item_id,hq,quantity,price_per_unit,timestamp FROM active_listing
                 WHERE retainer_id=$1 AND world_id=$2 AND item_id>0 AND quantity>0
                 AND price_per_unit BETWEEN 1 AND 999999999 ORDER BY timestamp DESC,id LIMIT 20",
                vec![row.try_get::<i32>("", "public_id")?.into(), world.into()],
            )
            .await?;
            let mut listings = Vec::new();
            let mut observed_at = None;
            for (slot, listing) in public.iter().enumerate() {
                let timestamp = listing.try_get::<NaiveDateTime>("", "timestamp")?.and_utc();
                observed_at =
                    Some(observed_at.map_or(timestamp, |old: DateTime<Utc>| old.max(timestamp)));
                listings.push(PrivateListing {
                    slot: slot as u8,
                    item_id: listing.try_get("", "item_id")?,
                    hq: listing.try_get("", "hq")?,
                    quantity: listing.try_get("", "quantity")?,
                    unit_price: i64::from(listing.try_get::<i32>("", "price_per_unit")?),
                });
            }
            (listings, observed_at, "universalis")
        };
        // One statement gives the comparison and its review timestamp the same
        // database snapshot. Exclude every retainer saved to this account.
        let items: Vec<i32> = listings.iter().map(|l| l.item_id).collect();
        let comparisons = query(&txn,
            "WITH requested AS (SELECT DISTINCT value::integer AS item_id FROM jsonb_array_elements_text($3::jsonb))
             SELECT i.item_id,q.hq,min(l.price_per_unit) AS cheapest,u.date_time
             FROM requested i CROSS JOIN (VALUES(false),(true)) q(hq)
             LEFT JOIN active_listing l ON l.world_id=$1 AND l.item_id=i.item_id AND l.hq=q.hq
               AND l.quantity>0 AND l.price_per_unit BETWEEN 1 AND 999999999
               AND NOT EXISTS(SELECT 1 FROM owned_retainers o WHERE o.discord_id=$2 AND o.retainer_id=l.retainer_id)
             LEFT JOIN listing_last_updated u ON u.world_id=$1 AND u.item_id=i.item_id
             GROUP BY i.item_id,q.hq,u.date_time",
            vec![world.into(),principal.owner.into(),serde_json::json!(items).into()]).await?;
        let mut prices = HashMap::new();
        for comparison in comparisons {
            prices.insert(
                (
                    comparison.try_get::<i32>("", "item_id")?,
                    comparison.try_get::<bool>("", "hq")?,
                ),
                (
                    comparison
                        .try_get::<Option<i32>>("", "cheapest")?
                        .map(i64::from),
                    comparison
                        .try_get::<Option<NaiveDateTime>>("", "date_time")?
                        .map(|t| t.and_utc()),
                ),
            );
        }
        let now = Utc::now();
        let mut quotes = Vec::with_capacity(listings.len());
        for listing in listings {
            let (competitor_price, market_checked_at) = prices
                .get(&(listing.item_id, listing.hq))
                .copied()
                .unwrap_or_default();
            let (suggested_price, status) = suggestion(competitor_price, market_checked_at, now);
            quotes.push(RetainerPriceQuote {
                slot: listing.slot,
                item_id: listing.item_id,
                hq: listing.hq,
                quantity: listing.quantity,
                current_price: listing.unit_price,
                competitor_price,
                market_checked_at,
                suggested_price,
                status: status.into(),
            });
        }
        let lookup = RetainerPriceLookup {
            retainer_id: retainer_id.to_string(),
            name: row.try_get("", "name")?,
            world_id: world,
            world_name: row.try_get("", "world_name")?,
            source: source.into(),
            observed_at,
            fetched_at: now,
            listings: quotes,
        };
        txn.commit().await?;
        Ok(lookup)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn suggestions_require_recent_market_review_and_valid_prices() {
        let now = Utc::now();
        assert_eq!(suggestion(Some(100), Some(now), now), (Some(99), "fresh"));
        assert_eq!(suggestion(Some(1), Some(now), now), (Some(1), "fresh"));
        assert_eq!(suggestion(Some(100), None, now), (None, "unknown_age"));
        for checked in [now - Duration::minutes(31), now + Duration::minutes(2)] {
            assert_eq!(suggestion(Some(100), Some(checked), now), (None, "stale"));
        }
        for price in [None, Some(0), Some(1_000_000_000)] {
            assert_eq!(suggestion(price, Some(now), now), (None, "no_competitors"));
        }
    }
}
