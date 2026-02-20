use crate::{
    UltrosDb,
    entity::{datacenter, region, world},
};
use anyhow::Result;
use sea_orm::{EntityTrait, ModelTrait};

impl UltrosDb {
    pub async fn get_all_worlds_regions_and_datacenters(
        &self,
    ) -> Result<(
        Vec<world::Model>,
        Vec<datacenter::Model>,
        Vec<region::Model>,
    )> {
        let worlds = world::Entity::find().all(&self.db).await?;
        let datacenters = datacenter::Entity::find().all(&self.db).await?;
        let regions = region::Entity::find().all(&self.db).await?;
        Ok((worlds, datacenters, regions))
    }

    pub async fn get_relative_worlds_datacenter_and_region(
        &self,
        world: &world::Model,
    ) -> Result<(Vec<world::Model>, datacenter::Model, region::Model)> {
        let datacenter = world
            .find_related(datacenter::Entity)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::Error::msg("Datacenter not found"))?;
        let region = datacenter
            .find_related(region::Entity)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::Error::msg("Region not found"))?;
        let worlds = datacenter.find_related(world::Entity).all(&self.db).await?;
        Ok((worlds, datacenter, region))
    }
}
