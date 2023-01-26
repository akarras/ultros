use std::rc::Rc;

use leptos::*;
use ultros_api_types::world_helper::WorldHelper;
#[derive(Clone)]
pub(crate) struct LocalWorldData(pub(crate) Resource<&'static str, Option<Rc<WorldHelper>>>);
