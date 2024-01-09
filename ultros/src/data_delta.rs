use async_trait::async_trait;

enum DataSelector {
    Minimum,
    Maximum,
}


#[async_trait]
trait DataDelta {
    fn force_load_data(&self, db: DataSelector) -> ();

}