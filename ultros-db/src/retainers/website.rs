//! Account-only projections. Uploaded observations never enter active_listing.
use crate::UltrosDb;
use crate::plugin::{exec, query};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::Value as Json;
use std::collections::{BTreeMap, HashMap};
use ultros_api_types::plugin::PrivateListing;
use ultros_api_types::user::{
    RetainerCharacter, RetainerListing, RetainerListingId, UserRetainerListings, UserRetainers,
};
use ultros_api_types::{ActiveListing, FfxivCharacter, Retainer};

struct Snapshot {
    id: i64,
    revision: i64,
    observed_at: DateTime<Utc>,
    listings: Vec<PrivateListing>,
    confirmed_slots: Vec<u8>,
}

/// Match identical stacks once each. A complete snapshot replaces older public
/// rows in this private view; public rows observed later remain visible. Once a
/// slot has been confirmed, its disappearance must not revive it as unverified.
fn listing_view(
    retainer: &Retainer,
    public: Vec<ActiveListing>,
    snapshot: &mut Snapshot,
) -> Vec<RetainerListing> {
    let mut public: Vec<_> = public.into_iter().map(Some).collect();
    let mut result = Vec::new();
    for row in &snapshot.listings {
        let matched = public.iter_mut().find(|listing| {
            listing.as_ref().is_some_and(|listing| {
                listing.item_id == row.item_id
                    && listing.hq == row.hq
                    && listing.quantity == row.quantity
                    && i64::from(listing.price_per_unit) == row.unit_price
                    // Older public rows cannot confirm a newer game capture.
                    // Repricing can temporarily withdraw those public rows;
                    // retiring the private slot on that basis loses listings.
                    && listing.timestamp >= snapshot.observed_at.naive_utc()
            })
        });
        if let Some(listing) = matched.and_then(Option::take) {
            if !snapshot.confirmed_slots.contains(&row.slot) {
                snapshot.confirmed_slots.push(row.slot);
            }
            result.push(listing.into());
        } else if !snapshot.confirmed_slots.contains(&row.slot) {
            result.push(RetainerListing {
                id: RetainerListingId::InGame {
                    revision: snapshot.revision,
                    slot: row.slot,
                },
                world_id: retainer.world_id,
                retainer_id: retainer.id,
                item_id: row.item_id,
                hq: row.hq,
                quantity: row.quantity,
                price_per_unit: row.unit_price as i32,
                timestamp: snapshot.observed_at.naive_utc(),
            });
        }
    }
    result.extend(
        public
            .into_iter()
            .flatten()
            .filter(|listing| listing.timestamp > snapshot.observed_at.naive_utc())
            .map(RetainerListing::from),
    );
    result
}

impl UltrosDb {
    pub async fn website_retainers(&self, owner: i64) -> Result<UserRetainers> {
        let observed = self.registered_game_characters(owner).await?;
        let mut game_characters = HashMap::new();
        for character in observed {
            let group = RetainerCharacter::Unverified {
                id: character.id,
                name: character.name,
                world_id: character.home_world,
            };
            for retainer in character.retainers {
                if let Some(id) = retainer.public_retainer_id {
                    game_characters.insert(id, group.clone());
                }
            }
        }
        let mut groups = BTreeMap::<_, Vec<_>>::new();
        for (character, retainers) in self
            .get_all_owned_retainers_and_character(owner as u64)
            .await?
        {
            let verified = character
                .map(|character| RetainerCharacter::Verified(FfxivCharacter::from(character)));
            for (owned, retainer) in retainers {
                let group = verified
                    .clone()
                    .or_else(|| game_characters.get(&retainer.id).cloned());
                groups
                    .entry(group)
                    .or_default()
                    .push((owned.into(), retainer.into()));
            }
        }
        Ok(UserRetainers {
            retainers: groups.into_iter().collect(),
        })
    }

    pub async fn website_retainer_listings(&self, owner: i64) -> Result<UserRetainerListings> {
        let groups = self.website_retainers(owner).await?;
        let mut public: HashMap<_, _> = self
            .get_retainer_listings_for_discord_user(owner as u64)
            .await?
            .into_iter()
            .map(|(_, retainer, listings)| {
                (
                    retainer.id,
                    listings
                        .into_iter()
                        .map(ActiveListing::from)
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        let mut snapshots = HashMap::new();
        // Both the registration and owned-row joins are owner-bound: removing a
        // retainer from the website also removes its private listing projection.
        for row in query(&self.db,
            "SELECT DISTINCT ON(p.public_retainer_id) s.*,p.public_retainer_id FROM plugin_snapshot s
             JOIN plugin_retainer p ON p.id=s.retainer_id AND p.owner=s.owner
             JOIN plugin_character c ON c.id=p.registered_character AND c.owner=p.owner AND c.registered
             JOIN owned_retainers o ON o.retainer_id=p.public_retainer_id AND o.discord_id=p.owner
             WHERE s.owner=$1 ORDER BY p.public_retainer_id,s.revision DESC", vec![owner.into()]).await? {
            snapshots.insert(row.try_get::<i32>("", "public_retainer_id")?, Snapshot {
                id: row.try_get("", "id")?,
                revision: row.try_get("", "revision")?,
                observed_at: row.try_get("", "observed_at")?,
                listings: serde_json::from_value(row.try_get::<Json>("", "listings")?)?,
                confirmed_slots: serde_json::from_value(row.try_get::<Json>("", "confirmed_slots")?)?,
            });
        }
        let mut result = Vec::new();
        for (character, retainers) in groups.retainers {
            let mut listings = Vec::new();
            for (_, retainer) in retainers {
                let public = public.remove(&retainer.id).unwrap_or_default();
                let rows = if let Some(snapshot) = snapshots.get_mut(&retainer.id) {
                    let before = snapshot.confirmed_slots.len();
                    let view = listing_view(&retainer, public, snapshot);
                    if snapshot.confirmed_slots.len() != before {
                        // Merge concurrent readers' confirmations, never overwrite.
                        exec(&self.db,
                            "UPDATE plugin_snapshot SET confirmed_slots=(SELECT jsonb_agg(DISTINCT value) FROM jsonb_array_elements(confirmed_slots || $3)) WHERE owner=$1 AND id=$2",
                            vec![owner.into(),snapshot.id.into(),serde_json::json!(snapshot.confirmed_slots).into()]).await?;
                    }
                    view
                } else {
                    public.into_iter().map(RetainerListing::from).collect()
                };
                listings.push((retainer, rows));
            }
            result.push((character, listings));
        }
        Ok(UserRetainerListings { retainers: result })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(id: i64, quantity: i32, price: i32, time: DateTime<Utc>) -> ActiveListing {
        ActiveListing {
            id,
            world_id: 1,
            retainer_id: 1,
            item_id: 8,
            hq: false,
            quantity,
            price_per_unit: price,
            timestamp: time.naive_utc(),
        }
    }
    #[test]
    fn captures_are_private_and_identical_stacks_confirm_once_each() {
        let time = Utc::now();
        let retainer = Retainer {
            id: 1,
            world_id: 1,
            name: "Retainer".into(),
            retainer_city_id: 1,
        };
        let mut snapshot = Snapshot {
            id: 1,
            revision: 1,
            observed_at: time,
            listings: vec![
                PrivateListing {
                    slot: 0,
                    item_id: 8,
                    hq: false,
                    quantity: 1,
                    unit_price: 50,
                },
                PrivateListing {
                    slot: 1,
                    item_id: 8,
                    hq: false,
                    quantity: 1,
                    unit_price: 50,
                },
            ],
            confirmed_slots: vec![],
        };
        let initial = listing_view(
            &retainer,
            vec![row(10, 1, 60, time - chrono::Duration::minutes(1))],
            &mut snapshot,
        );
        assert_eq!(initial.len(), 2);
        assert!(
            initial
                .iter()
                .all(|listing| matches!(listing.id, RetainerListingId::InGame { .. }))
        );
        let confirmed = listing_view(&retainer, vec![row(11, 1, 50, time)], &mut snapshot);
        assert_eq!(confirmed.len(), 2);
        assert!(matches!(
            confirmed[0].id,
            RetainerListingId::Universalis { id: 11 }
        ));
        assert!(matches!(
            confirmed[1].id,
            RetainerListingId::InGame { slot: 1, .. }
        ));
        assert_eq!(snapshot.confirmed_slots, vec![0]);
        let sold = listing_view(&retainer, vec![], &mut snapshot);
        assert_eq!(
            sold.len(),
            1,
            "a confirmed and subsequently removed stack must not reappear"
        );
        assert!(matches!(
            sold[0].id,
            RetainerListingId::InGame { slot: 1, .. }
        ));
        snapshot.listings.clear();
        assert!(
            listing_view(
                &retainer,
                vec![row(12, 1, 50, time - chrono::Duration::seconds(1))],
                &mut snapshot
            )
            .is_empty()
        );
        assert_eq!(
            listing_view(
                &retainer,
                vec![row(13, 1, 50, time + chrono::Duration::seconds(1))],
                &mut snapshot
            )
            .len(),
            1
        );
    }

    #[test]
    fn older_public_rows_cannot_retire_a_fresh_complete_game_capture() {
        let time = Utc::now();
        let retainer = Retainer {
            id: 1,
            world_id: 1,
            name: "Retainer".into(),
            retainer_city_id: 1,
        };
        let mut snapshot = Snapshot {
            id: 1,
            revision: 1,
            observed_at: time,
            listings: vec![
                PrivateListing {
                    slot: 0,
                    item_id: 8,
                    hq: false,
                    quantity: 1,
                    unit_price: 50,
                },
                PrivateListing {
                    slot: 1,
                    item_id: 8,
                    hq: false,
                    quantity: 1,
                    unit_price: 50,
                },
                PrivateListing {
                    slot: 2,
                    item_id: 8,
                    hq: false,
                    quantity: 2,
                    unit_price: 60,
                },
            ],
            confirmed_slots: vec![],
        };
        let before = time - chrono::Duration::seconds(10);
        let old_public = vec![
            row(10, 1, 50, before),
            row(11, 1, 50, before),
            row(12, 2, 60, before),
        ];
        let initial = listing_view(&retainer, old_public.clone(), &mut snapshot);
        assert_eq!(initial.len(), 3);
        assert!(
            initial
                .iter()
                .all(|l| matches!(l.id, RetainerListingId::InGame { .. }))
        );
        assert!(
            snapshot.confirmed_slots.is_empty(),
            "stale matches must not confirm a new upload"
        );

        // Adjusting prices withdraws public stacks one at a time. An older
        // public observation is not evidence that a freshly captured stack sold.
        for remaining in (0..=3).rev() {
            let current = listing_view(&retainer, old_public[..remaining].to_vec(), &mut snapshot);
            assert_eq!(current.len(), 3, "repricing lost an uploaded stack");
        }

        // A complete second game response changes one price, retaining the
        // identical pair and the untouched rows without duplication.
        snapshot.revision = 2;
        snapshot.observed_at = time + chrono::Duration::seconds(1);
        snapshot.listings[2].unit_price = 55;
        let repriced = listing_view(&retainer, old_public, &mut snapshot);
        assert_eq!(
            repriced
                .iter()
                .map(|l| l.price_per_unit)
                .collect::<Vec<_>>(),
            vec![50, 50, 55]
        );
        let confirmed = listing_view(
            &retainer,
            vec![row(13, 2, 55, time + chrono::Duration::seconds(2))],
            &mut snapshot,
        );
        assert_eq!(confirmed.len(), 3);
        assert_eq!(snapshot.confirmed_slots, vec![2]);
        assert_eq!(
            listing_view(&retainer, vec![], &mut snapshot).len(),
            2,
            "a subsequently confirmed sale must still disappear"
        );
    }
}
