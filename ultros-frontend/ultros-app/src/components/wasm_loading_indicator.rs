use super::loading::*;
use leptos::prelude::*;

#[component]
pub fn WasmLoadingIndicator() -> impl IntoView {
    // this is set to true on server or on client
    let (loading, set_loading) = signal(true);
    // create_effect only runs on the client, so we immediately
    // set `loading` to false if we're on the client
    Effect::new(move |_| {
        set_loading(false);
    });
    {
        move || {
            loading().then(|| {
                view! { <Loading/> }
            })
        }
    }
}
