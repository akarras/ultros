use std::sync::Arc;

use ultros_api_types::world_helper::WorldHelper;

use crate::error::AppResult;
#[derive(Clone)]
pub struct LocalWorldData(pub AppResult<Arc<WorldHelper>>);
