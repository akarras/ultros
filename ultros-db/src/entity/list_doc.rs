//! `SeaORM` Entity. Hand-authored for the local-first list document store.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "list_doc")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub list_id: i32,
    pub snapshot: Vec<u8>,
    pub version: Vec<u8>,
    pub changes_since_compaction: i32,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::list::Entity",
        from = "Column::ListId",
        to = "super::list::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    List,
}

impl Related<super::list::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::List.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
