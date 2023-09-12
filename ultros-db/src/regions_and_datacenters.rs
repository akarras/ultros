use std::collections::BTreeSet;

use crate::entity::*;
use crate::partial_diff_iterator::PartialDiffIterator;
use crate::UltrosDb;
use anyhow::Result;
use itertools::Itertools;
use migration::Order;
use sea_orm::{ActiveValue, EntityTrait, QueryOrder, Set};
use tracing::{error, info, instrument, warn};
use universalis::{DataCenterView, DataCentersView, WorldsView};
use universalis::{RegionName, WorldView};

impl PartialEq<datacenter::Model> for DataCenterView {
    fn eq(&self, other: &datacenter::Model) -> bool {
        self.name.0.eq(&other.name)
    }
}

impl PartialOrd<datacenter::Model> for DataCenterView {
    fn partial_cmp(&self, other: &datacenter::Model) -> Option<std::cmp::Ordering> {
        Some(self.name.0.cmp(&other.name))
    }
}

impl PartialEq<region::Model> for RegionName {
    fn eq(&self, other: &region::Model) -> bool {
        self.0.eq(&other.name)
    }
}

impl PartialOrd<region::Model> for RegionName {
    fn partial_cmp(&self, other: &region::Model) -> Option<std::cmp::Ordering> {
        Some(self.0.cmp(&other.name))
    }
}

impl PartialEq<world::Model> for WorldView {
    fn eq(&self, other: &world::Model) -> bool {
        self.name.0.eq(&other.name)
    }
}

impl PartialOrd<world::Model> for WorldView {
    fn partial_cmp(&self, other: &world::Model) -> Option<std::cmp::Ordering> {
        self.name.0.partial_cmp(&other.name)
    }
}

impl UltrosDb {
    #[instrument]
    pub async fn update_datacenters(
        &self,
        datacenter: &DataCentersView,
        worlds: &WorldsView,
    ) -> Result<()> {
        {
            let current_regions = region::Entity::find()
                .all(&self.db)
                .await?
                .into_iter()
                .sorted_by_cached_key(|c| c.name.to_string())
                .collect::<Vec<_>>();
            let new_regions: BTreeSet<RegionName> =
                datacenter.0.iter().map(|d| &d.region).cloned().collect();
            let diff = PartialDiffIterator::from((new_regions.iter(), current_regions.iter()));
            let added_regions: Vec<_> = diff
                .flat_map(|m| match m {
                    crate::partial_diff_iterator::DiffItem::Left(l) => Some(region::ActiveModel {
                        id: ActiveValue::default(),
                        name: Set(l.0.clone()),
                    }),
                    _ => None,
                })
                .collect();
            if !added_regions.is_empty() {
                tracing::warn!("new regions {added_regions:?}");
                if let Err(e) = region::Entity::insert_many(added_regions)
                    .exec(&self.db)
                    .await
                {
                    error!("Error updating regions {e}");
                }
            } else {
                info!("no new regions");
            }
        }
        {
            let regions = region::Entity::find().all(&self.db).await?;
            let existing_datacenters = datacenter::Entity::find().all(&self.db).await?;
            let new_datacenters: Vec<DataCenterView> = datacenter
                .0
                .iter()
                .sorted_by(|a, b| a.name.cmp(&b.name))
                .cloned()
                .collect();
            let new_datacenters: Vec<_> =
                PartialDiffIterator::from((new_datacenters.iter(), existing_datacenters.iter()))
                    .flat_map(|m| match m {
                        crate::partial_diff_iterator::DiffItem::Same(_, _) => None,
                        crate::partial_diff_iterator::DiffItem::Left(datacenter) => {
                            Some(datacenter::ActiveModel {
                                id: ActiveValue::default(),
                                name: Set(datacenter.name.0.clone()),
                                region_id: Set(regions
                                    .iter()
                                    .find(|r| r.name == datacenter.region.0)
                                    .map(|m| m.id)
                                    .expect("We should have all regions stored at this point.")),
                            })
                        }
                        crate::partial_diff_iterator::DiffItem::Right(_) => None,
                    })
                    .collect();
            if !new_datacenters.is_empty() {
                info!("new datacenters {new_datacenters:?}");
                if let Err(e) = datacenter::Entity::insert_many(new_datacenters)
                    .exec(&self.db)
                    .await
                {
                    error!("Error updating datacenters {e}");
                }
            } else {
                info!("no new datacenters");
            }
        }
        {
            let datacenters = datacenter::Entity::find().all(&self.db).await?;
            let mut existing_worlds = world::Entity::find().all(&self.db).await?;
            let worlds: Vec<_> = worlds
                .0
                .iter()
                .sorted_by(|a, b| a.name.cmp(&b.name))
                .cloned()
                .collect();
            existing_worlds.sort_by(|a, b| a.name.cmp(&b.name));
            let worlds: Vec<_> = PartialDiffIterator::from((worlds.iter(), existing_worlds.iter()))
                .flat_map(|m| match m {
                    crate::partial_diff_iterator::DiffItem::Same(_, _) => None,
                    crate::partial_diff_iterator::DiffItem::Left(left) => {
                        Some(world::ActiveModel {
                            id: Set(left.id.0),
                            name: Set(left.name.0.clone()),
                            datacenter_id: Set(datacenter
                                .0
                                .iter()
                                .find(|d| d.worlds.iter().any(|w| *w == left.id))
                                .and_then(|m| {
                                    datacenters
                                        .iter()
                                        .find(|dc| dc.name == m.name.0)
                                        .map(|m| m.id)
                                })
                                .expect("Should have a valid datacenter id available")),
                        })
                    }
                    crate::partial_diff_iterator::DiffItem::Right(_right) => None,
                })
                .collect();
            if !worlds.is_empty() {
                info!("new worlds {worlds:?}");
                if let Err(e) = world::Entity::insert_many(worlds).exec(&self.db).await {
                    error!("Error inserting worlds {e}");
                }
            } else {
                info!("no new worlds");
            }
        }

        Ok(())
    }
}
