#[cfg(not(test))]
#[tokio::main]
async fn main() {
    // use sea_orm_migration::prelude::*;
    // todo: fixup after sea orm update
    // cli::run_cli(migration::Migrator).await;
}

#[cfg(test)]
fn main() {}
