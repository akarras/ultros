use crate::api::get_login;
use leptos::*;

#[component]
pub fn ProfileDisplay(cx: Scope) -> impl IntoView {
    let (login, set_login) = create_signal(cx, None);
    spawn_local(async move {
        let login = get_login(cx).await;
        leptos::log!("login {login:?}");
        set_login(Some(login));
    });
    {
        move || match login() {
            Some(Ok(auth)) => view! {cx,
            <a href="/profile">
                <img class="avatar" src=&auth.avatar alt=&auth.username/>
            </a>
            <a rel="external" class="btn" href="/logout">
                "Logout"
            </a>}
            .into_view(cx),
            _ => view! {cx, <a rel="external" class="btn" href="/login">
                <i class="fa-brands fa-discord"></i>"Login"
            </a>
            }
            .into_view(cx),
        }
    }
}
