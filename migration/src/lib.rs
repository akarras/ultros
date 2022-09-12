pub use sea_orm_migration::prelude::*;

mod m20220101_000001_create_table;
mod m20220908_111724_create_index;
mod m20220908_170456_add_world_id_to_sale;
mod m20220911_182657_add_character_verification_tables;
mod m20220911_200503_add_sale_index;

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
        ]
    }
}
