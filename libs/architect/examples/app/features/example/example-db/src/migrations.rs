//! SeaORM migrator + concrete migrations for the `examples` table.

use sea_orm_migration::prelude::*;

/// The migrator the `apps/db` binary and integration tests run.
pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(m20260512_000001_create_examples::Migration)]
    }
}

// ── Migrations ─────────────────────────────────────────────────────────

mod m20260512_000001_create_examples {
    use sea_orm_migration::prelude::*;

    #[derive(DeriveMigrationName)]
    pub struct Migration;

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
            manager
                .create_table(
                    Table::create()
                        .table(Examples::Table)
                        .if_not_exists()
                        .col(ColumnDef::new(Examples::Id).uuid().not_null().primary_key())
                        .col(ColumnDef::new(Examples::Name).string().not_null())
                        .col(ColumnDef::new(Examples::Description).text().not_null())
                        .col(
                            ColumnDef::new(Examples::CreatedAt)
                                .timestamp_with_time_zone()
                                .not_null(),
                        )
                        .col(
                            ColumnDef::new(Examples::UpdatedAt)
                                .timestamp_with_time_zone()
                                .not_null(),
                        )
                        .to_owned(),
                )
                .await
        }

        async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
            manager
                .drop_table(Table::drop().table(Examples::Table).to_owned())
                .await
        }
    }

    #[derive(Iden)]
    enum Examples {
        Table,
        Id,
        Name,
        Description,
        CreatedAt,
        UpdatedAt,
    }
}
