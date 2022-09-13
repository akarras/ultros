pub use sea_orm_migration::prelude::*;

mod m20220101_000001_create_table;
mod m20220908_111724_create_index;
mod m20220908_170456_add_world_id_to_sale;
mod m20220911_182657_add_character_verification_tables;
mod m20220911_200503_add_sale_index;
mod m20220912_194248_joint_foreign_key_world_retainer;
mod m20220912_201555_drop_retainer_universalis_id;
mod m20220912_202909_drop_old_listing_retainer_fk;
mod m20220913_102706_retainer_id_before_world_id_in_foreign_key;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20220101_000001_create_table::Migration),
            Box::new(m20220908_111724_create_index::Migration),
            Box::new(m20220908_170456_add_world_id_to_sale::Migration),
            Box::new(m20220911_182657_add_character_verification_tables::Migration),
            Box::new(m20220911_200503_add_sale_index::Migration),
            Box::new(m20220912_194248_joint_foreign_key_world_retainer::Migration),
            Box::new(m20220912_201555_drop_retainer_universalis_id::Migration),
            Box::new(m20220912_202909_drop_old_listing_retainer_fk::Migration),
            Box::new(m20220913_102706_retainer_id_before_world_id_in_foreign_key::Migration),
        ]
    }
}
