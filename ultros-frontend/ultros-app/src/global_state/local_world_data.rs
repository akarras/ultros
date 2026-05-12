use std::sync::Arc;

use ultros_api_types::world_helper::WorldHelper;

use crate::error::{AppError, AppResult, SystemError};
#[derive(Clone)]
pub struct LocalWorldData(pub AppResult<Arc<WorldHelper>>);

impl LocalWorldData {
    pub fn failed(message: impl Into<String>) -> Self {
        Self(Err(AppError::SystemError(SystemError::Message(
            message.into(),
        ))))
    }
}
