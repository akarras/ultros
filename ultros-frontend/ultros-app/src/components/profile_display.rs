use crate::api::get_login;
use leptos::*;
use leptos_router::*;

#[component]
pub fn ProfileDisplay(cx: Scope) -> impl IntoView {
    let auth = create_resource(cx, || (), move |_| async move { get_login(cx).await });
    view! {cx,
        <div>
            <Suspense fallback=move || view!{cx, <div class="loading"></div>}>
            {move || {
                match auth.read() {
                    Some(Some(auth)) => {
                        view!{cx,
                            <A href="profile">
                                <img class="avatar" src=&auth.avatar alt=&auth.username/>
                            </A>
                            <A href="logout">
                                "Logout"
                            </A>}.into_view(cx)
                    }
                    _ => {
                        view!{cx, <A href="login">
                            "Login"
                        </A>
                        }.into_view(cx)
                    }
                }
            }}
            </Suspense>
        </div>
    }
}
