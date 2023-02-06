use sea_orm_migration::{prelude::*, sea_orm::StatementBuilder, sea_query::ColumnDef};

#[derive(DeriveMigrationName)]
pub struct Migration;

struct TimeScaleInstall;

impl StatementBuilder for TimeScaleInstall {
    fn build(&self, db_backend: &sea_orm::DbBackend) -> sea_orm::Statement {
        sea_orm::Statement::from_string(
            *db_backend,
            "CREATE EXTENSION IF NOT EXISTS timescaledb CASCADE;".to_string(),
        )
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // println!("{:?}", manager.exec_stmt(TimeScaleInstall).await);
        manager
            .create_table(
                Table::create()
                    .table(DiscordUser::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DiscordUser::Id)
                            .big_unsigned()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(DiscordUser::Username).string().not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(Alert::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alert::Id)
                            .integer()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alert::Owner).big_unsigned().not_null())
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(RetainerCity::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(RetainerCity::Id)
                            .integer()
                            .primary_key()
                            .not_null(),
                    )
                    .col(ColumnDef::new(RetainerCity::Name).string().not_null())
                    .to_owned(),
            )
            .await?;

        manager
            .create_foreign_key(
                ForeignKey::create()
                    .on_delete(ForeignKeyAction::Cascade)
                    .from(Alert::Table, Alert::Owner)
                    .to(DiscordUser::Table, DiscordUser::Id)
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(Retainer::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Retainer::Id)
                            .integer()
                            .primary_key()
                            .auto_increment()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Retainer::UniversalisID)
                            .string()
                            .unique_key()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Retainer::WorldId).integer().not_null())
                    .col(ColumnDef::new(Retainer::Name).string().not_null())
                    .col(
                        ColumnDef::new(Retainer::RetainerCityId)
                            .integer()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKeyCreateStatement::new()
                    .from(Retainer::Table, Retainer::RetainerCityId)
                    .to(RetainerCity::Table, RetainerCity::Id)
                    .on_delete(ForeignKeyAction::Cascade)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(World::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(World::Id)
                            .integer()
                            .primary_key()
                            .not_null()
                            .auto_increment(),
                    )
                    .col(ColumnDef::new(World::Name).unique_key().string().not_null())
                    .col(ColumnDef::new(World::DatacenterId).integer().not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(Datacenter::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Datacenter::Id)
                            .integer()
                            .not_null()
                            .primary_key()
                            .auto_increment(),
                    )
                    .col(
                        ColumnDef::new(Datacenter::Name)
                            .unique_key()
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Datacenter::RegionId).integer().not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKeyCreateStatement::new()
                    .from(World::Table, World::DatacenterId)
                    .to(Datacenter::Table, Datacenter::Id)
                    .on_delete(ForeignKeyAction::Cascade)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(Region::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Region::Id)
                            .primary_key()
                            .not_null()
                            .auto_increment()
                            .integer(),
                    )
                    .col(
                        ColumnDef::new(Region::Name)
                            .unique_key()
                            .string()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKeyCreateStatement::new()
                    .from(Datacenter::Table, Datacenter::RegionId)
                    .to(Region::Table, Region::Id)
                    .on_update(ForeignKeyAction::Cascade)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(FinalFantasyCharacter::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(FinalFantasyCharacter::Id)
                            .primary_key()
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(FinalFantasyCharacter::FirstName)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(FinalFantasyCharacter::LastName)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(FinalFantasyCharacter::WorldId)
                            .integer()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKeyCreateStatement::new()
                    .from(FinalFantasyCharacter::Table, FinalFantasyCharacter::WorldId)
                    .to(World::Table, World::Id)
                    .on_update(ForeignKeyAction::Cascade)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(OwnedRetainers::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(OwnedRetainers::Id)
                            .primary_key()
                            .integer()
                            .not_null()
                            .auto_increment(),
                    )
                    .col(
                        ColumnDef::new(OwnedRetainers::RetainerId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OwnedRetainers::DiscordId)
                            .big_unsigned()
                            .not_null(),
                    )
                    .col(ColumnDef::new(OwnedRetainers::CharacterId).integer())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                IndexCreateStatement::new()
                    .table(OwnedRetainers::Table)
                    .col(OwnedRetainers::DiscordId)
                    .col(OwnedRetainers::RetainerId)
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKeyCreateStatement::new()
                    .from(OwnedRetainers::Table, OwnedRetainers::DiscordId)
                    .to(DiscordUser::Table, DiscordUser::Id)
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKeyCreateStatement::new()
                    .from(OwnedRetainers::Table, OwnedRetainers::RetainerId)
                    .to(Retainer::Table, Retainer::Id)
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKeyCreateStatement::new()
                    .from(OwnedRetainers::Table, OwnedRetainers::CharacterId)
                    .to(FinalFantasyCharacter::Table, FinalFantasyCharacter::Id)
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(UnknownFinalFantasyCharacter::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(UnknownFinalFantasyCharacter::Id)
                            .integer()
                            .primary_key()
                            .auto_increment()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(UnknownFinalFantasyCharacter::Name)
                            .string()
                            .unique_key()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(SaleHistory::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(SaleHistory::Id).auto_increment().integer())
                    .col(ColumnDef::new(SaleHistory::Quantity).integer().not_null())
                    .col(
                        ColumnDef::new(SaleHistory::PricePerItem)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(SaleHistory::BuyingCharacterId)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(SaleHistory::Hq).boolean().not_null())
                    .col(ColumnDef::new(SaleHistory::SoldItemId).integer().not_null())
                    .col(ColumnDef::new(SaleHistory::SoldDate).date_time().not_null())
                    .primary_key(
                        Index::create()
                            .col(SaleHistory::Id)
                            .col(SaleHistory::SoldDate),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKeyCreateStatement::new()
                    .from(SaleHistory::Table, SaleHistory::BuyingCharacterId)
                    .to(
                        UnknownFinalFantasyCharacter::Table,
                        UnknownFinalFantasyCharacter::Id,
                    )
                    .on_delete(ForeignKeyAction::Cascade)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(MateriaListing::Table)
                    .col(
                        ColumnDef::new(MateriaListing::Id)
                            .integer()
                            .primary_key()
                            .not_null()
                            .auto_increment(),
                    )
                    .col(ColumnDef::new(MateriaListing::MateriaId).integer())
                    .col(ColumnDef::new(MateriaListing::Slot).small_integer())
                    .col(
                        ColumnDef::new(MateriaListing::ActiveListingId)
                            .integer()
                            .unique_key()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(ActiveListing::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ActiveListing::Id)
                            .integer()
                            .not_null()
                            .unique_key()
                            .auto_increment(),
                    )
                    .col(ColumnDef::new(ActiveListing::WorldId).integer().not_null())
                    .col(ColumnDef::new(ActiveListing::ItemId).integer().not_null())
                    .col(
                        ColumnDef::new(ActiveListing::RetainerId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ActiveListing::PricePerUnit)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(ActiveListing::Quantity).integer().not_null())
                    .col(ColumnDef::new(ActiveListing::Hq).boolean().not_null())
                    .col(
                        ColumnDef::new(ActiveListing::Timestamp)
                            .date_time()
                            .not_null(),
                    )
                    .primary_key(
                        IndexCreateStatement::new()
                            .col(ActiveListing::Timestamp)
                            .col(ActiveListing::Id),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKeyCreateStatement::new()
                    .from(MateriaListing::Table, MateriaListing::ActiveListingId)
                    .to(ActiveListing::Table, ActiveListing::Id)
                    .on_update(ForeignKeyAction::Cascade)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKeyCreateStatement::new()
                    .from(ActiveListing::Table, ActiveListing::WorldId)
                    .to(World::Table, World::Id)
                    .on_delete(ForeignKeyAction::Cascade)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKeyCreateStatement::new()
                    .from(ActiveListing::Table, ActiveListing::RetainerId)
                    .to(Retainer::Table, Retainer::Id)
                    .on_update(ForeignKeyAction::Cascade)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKeyCreateStatement::new()
                    .from(Retainer::Table, Retainer::WorldId)
                    .to(World::Table, World::Id)
                    .on_update(ForeignKeyAction::Cascade)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(AlertRetainerUndercut::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AlertRetainerUndercut::Id)
                            .integer()
                            .primary_key()
                            .auto_increment()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AlertRetainerUndercut::AlertId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AlertRetainerUndercut::MarginPercent)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AlertRetainerUndercut::RetainerId)
                            .integer()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKeyCreateStatement::new()
                    .from(
                        AlertRetainerUndercut::Table,
                        AlertRetainerUndercut::RetainerId,
                    )
                    .to(Retainer::Table, Retainer::Id)
                    .on_update(ForeignKeyAction::Cascade)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKeyCreateStatement::new()
                    .from(AlertRetainerUndercut::Table, AlertRetainerUndercut::AlertId)
                    .to(Alert::Table, Alert::Id)
                    .on_delete(ForeignKeyAction::Cascade)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(AlertDiscordDestination::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AlertDiscordDestination::Id)
                            .integer()
                            .primary_key()
                            .auto_increment()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AlertDiscordDestination::AlertId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AlertDiscordDestination::ChannelId)
                            .big_unsigned()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKeyCreateStatement::new()
                    .from(
                        AlertDiscordDestination::Table,
                        AlertDiscordDestination::AlertId,
                    )
                    .to(Alert::Table, Alert::Id)
                    .on_update(ForeignKeyAction::Cascade)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;
        // create hypertable from sale history data. This can be compressed & aggregated instead of removed after a threshold
        // println!(
        //     "{:?}",
        //     manager
        //         .exec_stmt(
        //             SelectStatement::new()
        //                 .expr(Func::cust(CreateHypertable).args(vec![
        //                     SimpleExpr::Custom("'sale_history'".to_string()),
        //                     SimpleExpr::Custom("'sold_date'".to_string()),
        //                 ]))
        //                 .to_owned(),
        //         )
        //         .await
        // );

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(RetainerCity::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(AlertDiscordDestination::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(AlertRetainerUndercut::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(Alert::Table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(SaleHistory::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(MateriaListing::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(ActiveListing::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(OwnedRetainers::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(Retainer::Table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(FinalFantasyCharacter::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(UnknownFinalFantasyCharacter::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(World::Table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(Datacenter::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(Region::Table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(DiscordUser::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

struct CreateHypertable;

impl Iden for CreateHypertable {
    fn unquoted(&self, s: &mut dyn Write) {
        write!(s, "create_hypertable").unwrap();
    }
}

/// Learn more at https://docs.rs/sea-query#iden
#[derive(Iden)]
pub(crate) enum DiscordUser {
    Table,
    Id,
    Username,
}

#[derive(Iden)]
pub(crate) enum FinalFantasyCharacter {
    Table,
    Id,
    FirstName,
    LastName,
    WorldId,
}

#[derive(Iden)]
enum UnknownFinalFantasyCharacter {
    Table,
    Id,
    Name,
}

#[derive(Iden)]
pub enum OwnedRetainers {
    Table,
    Id,
    RetainerId,
    DiscordId,
    CharacterId,
}

#[derive(Iden)]
enum Alert {
    Table,
    Id,
    Owner,
}

#[derive(Iden)]
pub(crate) enum AlertRetainerUndercut {
    Table,
    Id,
    AlertId,
    // dropped in m20220916_011325_drop_retainer_id
    RetainerId,
    MarginPercent,
}

#[derive(Iden)]
enum AlertDiscordDestination {
    Table,
    Id,
    AlertId,
    ChannelId,
}

#[derive(Iden)]
pub(crate) enum ActiveListing {
    Table,
    Id,
    ItemId,
    PricePerUnit,
    Quantity,
    Hq,
    RetainerId,
    WorldId,
    Timestamp,
}

#[derive(Iden)]
pub(crate) enum MateriaListing {
    Table,
    Id,
    MateriaId,
    Slot,
    ActiveListingId,
}

#[derive(Iden)]
pub(crate) enum SaleHistory {
    Table,
    Id,
    SoldItemId,
    Quantity,
    Hq,
    PricePerItem,
    BuyingCharacterId,
    BuyerName,
    SoldDate,
    WorldId,
}

#[derive(Iden)]
pub(crate) enum World {
    Table,
    Id,
    Name,
    DatacenterId,
}

#[derive(Iden)]
pub(crate) enum Datacenter {
    Table,
    Id,
    Name,
    RegionId,
}

#[derive(Iden)]
pub(crate) enum Region {
    Table,
    Id,
    Name,
}

#[derive(Iden)]
pub(crate) enum Retainer {
    Table,
    Id,
    UniversalisID,
    WorldId,
    Name,
    RetainerCityId,
}

#[derive(Iden)]
pub(crate) enum RetainerCity {
    Table,
    Id,
    Name,
}
