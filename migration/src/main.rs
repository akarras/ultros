use sea_orm_migration::prelude::*;

#[cfg(not(test))]
#[tokio::main]
async fn main() {
    cli::run_cli(migration::Migrator).await;
}

#[cfg(test)]
fn main() {}
