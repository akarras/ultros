use std::rc::Rc;

use leptos::*;
use ultros_api_types::world_helper::WorldHelper;

use crate::api::get_worlds;

#[derive(Clone)]
pub(crate) struct LocalWorldData(pub(crate) Resource<(), Option<Rc<WorldHelper>>>);

impl LocalWorldData {
    pub(crate) fn new(cx: Scope) -> Self {
        let resource = create_resource(
            cx,
            move || {},
            move |_| async move {
                let world_data = get_worlds(cx).await;
                world_data.map(|data| Rc::new(WorldHelper::new(data)))
            },
        );
        Self(resource)
    }
}
