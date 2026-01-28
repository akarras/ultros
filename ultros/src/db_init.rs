use anyhow::Result;
use tracing::info;
use ultros_db::UltrosDb;
use universalis::{DataCentersView, WorldsView};

pub(crate) async fn init_db(
    db: &UltrosDb,
    worlds_view: Result<WorldsView, universalis::Error>,
    datacenters: Result<DataCentersView, universalis::Error>,
) -> Result<()> {
    info!("db starting");

    db.insert_default_retainer_cities().await.unwrap();
    info!("DB connected & ffxiv world data primed");
    {
        if let (Ok(worlds), Ok(datacenters)) = (worlds_view, datacenters) {
            db.update_datacenters(&datacenters, &worlds).await?;
        }
    }
    Ok(())
}
