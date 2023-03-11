use super::loading::*;
use leptos::{leptos_dom::console_log, *};

#[component]
pub fn WasmLoadingIndicator(cx: Scope) -> impl IntoView {
    // this is set to true on server or on client
    let (loading, set_loading) = create_signal(cx, true);
    // create_effect only runs on the client, so we immediately
    // set `loading` to false if we're on the client
    create_effect(cx, move |_| {
        set_loading(false);
        console_log("Loading done");
    });
    view! { cx,
      // <Show when=loading fallback=|_| {}>
      //   <Loading/>
      // </Show>
      {move || {
        loading().then(|| {
          view!{cx, <Loading/>}
        })
      }}
    }
}
