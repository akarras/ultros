use crate::{
    entity::{datacenter, region, world},
    UltrosDb,
};
use anyhow::Result;
use sea_orm::ModelTrait;

impl UltrosDb {
    pub async fn get_relative_worlds_datacenter_and_region(
        &self,
        world: &world::Model,
    ) -> Result<(Vec<world::Model>, datacenter::Model, region::Model)> {
        let datacenter = world
            .find_related(datacenter::Entity)
            .one(&self.db)
            .await?
            .ok_or(anyhow::Error::msg("Datacenter not found"))?;
        let region = datacenter
            .find_related(region::Entity)
            .one(&self.db)
            .await?
            .ok_or(anyhow::Error::msg("Region not found"))?;
        let worlds = datacenter.find_related(world::Entity).all(&self.db).await?;
        Ok((worlds, datacenter, region))
    }
}
