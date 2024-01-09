use std::sync::Arc;

use ultros_api_types::world_helper::WorldHelper;

use crate::error::AppResult;
#[derive(Clone)]
pub(crate) struct LocalWorldData(pub(crate) AppResult<Arc<WorldHelper>>);
