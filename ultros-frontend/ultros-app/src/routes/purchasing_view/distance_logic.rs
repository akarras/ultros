use ultros_api_types::world_helper::{AnyResult, WorldHelper};

pub(crate) fn get_teleport_cost(
    world_helper: &WorldHelper,
    source_world_id: i32,
    destination_world_id: i32,
) -> i32 {
    if source_world_id == destination_world_id {
        return 0;
    }
    let source = world_helper
        .lookup_selector(ultros_api_types::world_helper::AnySelector::World(
            source_world_id,
        ))
        .unwrap();
    let destination = world_helper
        .lookup_selector(ultros_api_types::world_helper::AnySelector::World(
            destination_world_id,
        ))
        .unwrap();
    let source_dc = match source {
        AnyResult::World(w) => Some(w.datacenter_id),
        _ => None,
    }
    .unwrap();
    let destination_dc = match destination {
        AnyResult::World(w) => Some(w.datacenter_id),
        _ => None,
    }
    .unwrap();

    if source_dc == destination_dc {
        100
    } else {
        1000
    }
}
