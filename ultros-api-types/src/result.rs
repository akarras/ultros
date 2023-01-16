use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
pub struct JsonError {
    pub error_message: String,
}
