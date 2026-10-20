use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute_unprepared(
            "ALTER TABLE plugin_snapshot ADD COLUMN confirmed_slots JSONB NOT NULL DEFAULT '[]';
             INSERT INTO retainer_city(id,name) VALUES(0,'Unknown') ON CONFLICT(id) DO NOTHING;
             INSERT INTO retainer(name,world_id,retainer_city_id)
             SELECT DISTINCT ON(c.home_world,p.name) p.name,c.home_world,coalesce(p.city,0)
             FROM plugin_retainer p JOIN plugin_character c ON c.id=p.registered_character AND c.owner=p.owner
             WHERE c.registered ORDER BY c.home_world,p.name,p.id
             ON CONFLICT(world_id,name) DO NOTHING;
             UPDATE plugin_retainer p SET public_retainer_id=r.id
             FROM plugin_character c,retainer r
             WHERE c.id=p.registered_character AND c.owner=p.owner AND c.registered
             AND r.name=p.name AND r.world_id=c.home_world AND p.public_retainer_id IS NULL
             AND NOT EXISTS(SELECT 1 FROM plugin_retainer other WHERE other.owner=p.owner AND other.public_retainer_id=r.id)
             AND NOT EXISTS(SELECT 1 FROM plugin_retainer other JOIN plugin_character oc ON oc.id=other.registered_character AND oc.owner=other.owner
               WHERE other.owner=p.owner AND other.id<>p.id AND other.name=p.name AND oc.home_world=c.home_world);
             INSERT INTO owned_retainers(discord_id,retainer_id,weight)
             SELECT p.owner,p.public_retainer_id,coalesce((SELECT max(weight) FROM owned_retainers o WHERE o.discord_id=p.owner),0)+row_number() OVER(PARTITION BY p.owner ORDER BY p.id)::integer
             FROM plugin_retainer p WHERE p.registered_character IS NOT NULL AND p.public_retainer_id IS NOT NULL
             AND NOT EXISTS(SELECT 1 FROM owned_retainers o WHERE o.discord_id=p.owner AND o.retainer_id=p.public_retainer_id)
             ON CONFLICT(discord_id,retainer_id) DO NOTHING;"
        ).await?;
        Ok(())
    }
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("ALTER TABLE plugin_snapshot DROP COLUMN confirmed_slots;")
            .await?;
        // Retainer identities and user registrations are intentionally retained.
        Ok(())
    }
}
