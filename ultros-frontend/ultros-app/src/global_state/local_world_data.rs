use std::rc::Rc;

use leptos::*;
use ultros_api_types::world_helper::WorldHelper;

use crate::error::AppResult;
#[derive(Clone)]
pub(crate) struct LocalWorldData(pub(crate) Resource<&'static str, AppResult<Rc<WorldHelper>>>);
