use std::collections::BTreeMap;
use universalis::{DataCenterName, RegionName, UniversalisClient, WorldName};

/// Holds data that changes infrequently. This data is initialized on initial load of the app. This could also come from
#[derive(Default, Debug)]
pub struct UniversalisData {
    pub regions: BTreeMap<RegionName, Vec<DataCenterName>>,
    pub data_centers: BTreeMap<DataCenterName, Vec<WorldName>>,
}

impl UniversalisData {
    pub async fn initialize_data() -> Self {
        let client = UniversalisClient::new();
        let (data_centers, worlds) =
            futures::future::join(client.get_data_centers(), client.get_worlds()).await;
        let worlds = worlds.unwrap();
        let data_centers = data_centers.unwrap();
        let regions: BTreeMap<RegionName, Vec<DataCenterName>> =
            data_centers.0.iter().fold(BTreeMap::new(), |mut map, dc| {
                map.entry(dc.region.clone())
                    .or_default()
                    .push(dc.name.clone());
                map
            });
        let data_centers = data_centers
            .0
            .into_iter()
            .map(|m| {
                (
                    m.name,
                    m.worlds
                        .into_iter()
                        .map(|m| {
                            worlds
                                .0
                                .iter()
                                .find(|world| world.id == m)
                                .map(|m| m.name.clone())
                                .unwrap()
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        Self {
            data_centers,
            regions,
        }
    }
}
