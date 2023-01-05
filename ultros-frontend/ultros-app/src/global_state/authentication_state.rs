use crate::api::get_login;
use leptos::*;
use serde::{Deserialize, Serialize};
use std::rc::Rc;
use ultros_api_types::user_data::UserData;

#[derive(Clone)]
pub(crate) struct AuthenticationState(pub(crate) Resource<(), Option<Rc<UserData>>>);

impl AuthenticationState {
    pub(crate) fn new(cx: Scope) -> Self {
        let resource = create_resource(
            cx,
            || (),
            move |_| async move {
                let login: Option<UserData> = get_login(cx).await;
                login.map(|login| Rc::new(login))
            },
        );
        Self(resource)
    }
}
