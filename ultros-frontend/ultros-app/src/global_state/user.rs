use leptos::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct LoggedInUser(ReadSignal<User>);

impl AsRef<ReadSignal<User>> for LoggedInUser {
    fn as_ref(&self) -> &ReadSignal<User> {
        &self.0
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct User {
    pub id: u64,
    pub username: String,
    pub avatar: String,
}
