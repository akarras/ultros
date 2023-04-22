use crate::{components::loading::Loading, global_state::user::LoggedInUser};
use leptos::*;

#[component]
pub fn ProfileDisplay(cx: Scope) -> impl IntoView {
    // let (login, set_login) = create_signal(cx, None);
    // spawn_local(async move {
    //     let login = get_login(cx).await;
    //     leptos::log!("login {login:?}");
    //     set_login(Some(login));
    // });
    let user = use_context::<LoggedInUser>(cx)
        .expect("Logged in user state to be present")
        .0;
    view! {cx,
        <Suspense fallback=move || view!{cx, <Loading/>}>
        {move || user.read(cx).map(|user| match user {
            Some(auth) => view! {cx,
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
        })}
        </Suspense>
    }
}
