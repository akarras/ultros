use crate::api::get_login;
use leptos::*;

#[component]
pub fn ProfileDisplay(cx: Scope) -> impl IntoView {
    let (login, set_login) = create_signal(cx, None);
    spawn_local(async move {
        let login = get_login(cx).await;
        leptos::log!("login {login:?}");
        set_login(login);
    });
    // let auth = create_resource(cx, || (), move |_| async move { get_login(cx).await });
    view! {cx,
        {move || match login() {
            Some(auth) => {
                view!{cx,
                    <html::a href="/profile">
                        <img class="avatar" src=&auth.avatar alt=&auth.username/>
                    </html::a>
                    <html::a class="btn" href="/logout">
                        "Logout"
                    </html::a>}.into_view(cx)
            }
            _ => {
                view!{cx, <html::a class="btn" href="/login">
                    "Login"
                </html::a>
                }.into_view(cx)
            }
        }}
    }
}
