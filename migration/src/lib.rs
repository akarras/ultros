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
mod m20220916_011325_drop_retainer_id_from_retainer_undercut;
mod m20220918_135945_add_item_id_and_retainer_indexes;
mod m20220918_203336_add_retainer_index_to_active_listing;
mod m20221003_151617_add_time_index;
mod m20221015_143611_sale_history_index;
mod m20221031_185333_add_order_column;
mod m20221112_211028_add_final_fantasy_character_relation_to_verification;
mod m20221227_164853_price_alert;
mod m20230311_230232_cleanup_fks;
mod m20230312_184157_add_last_updated_table;
mod m20230918_033709_remove_junk_pk_from_listing;
mod m20240203_230653_add_list_item_owned_quantity;
mod m20240424_000001_create_notification_endpoints;

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
            Box::new(m20220916_011325_drop_retainer_id_from_retainer_undercut::Migration),
            Box::new(m20220918_135945_add_item_id_and_retainer_indexes::Migration),
            Box::new(m20220918_203336_add_retainer_index_to_active_listing::Migration),
            Box::new(m20221003_151617_add_time_index::Migration),
            Box::new(m20221015_143611_sale_history_index::Migration),
            Box::new(m20221031_185333_add_order_column::Migration),
            Box::new(
                m20221112_211028_add_final_fantasy_character_relation_to_verification::Migration,
            ),
            Box::new(m20221227_164853_price_alert::Migration),
            Box::new(m20230311_230232_cleanup_fks::Migration),
            Box::new(m20230312_184157_add_last_updated_table::Migration),
            Box::new(m20230918_033709_remove_junk_pk_from_listing::Migration),
            Box::new(m20240203_230653_add_list_item_owned_quantity::Migration),
            Box::new(m20240424_000001_create_notification_endpoints::Migration),
        ]
    }
}
