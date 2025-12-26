use leptos::prelude::*;
use crate::api::trigger_rescan;

#[component]
pub fn Admin() -> impl IntoView {
    let trigger_rescan = Action::new(|_: &()| async {
        if let Err(e) = trigger_rescan().await {
            tracing::error!("Failed to rescan: {e}");
        }
    });

    view! {
        <div class="p-4 space-y-4">
            <h1 class="text-2xl font-bold">"Admin Panel"</h1>
            <div class="card p-4 space-y-2">
                <h2 class="text-xl font-semibold">"Analyzer Service"</h2>
                <p>"Manually trigger a full rescan of the analyzer service from the database."</p>
                <button
                    class="btn"
                    on:click=move |_| trigger_rescan.dispatch(())
                    disabled=move || trigger_rescan.pending().get()
                >
                    {move || if trigger_rescan.pending().get() {
                        "Rescanning..."
                    } else {
                        "Trigger Rescan"
                    }}
                </button>
                {move || trigger_rescan.value().get().map(|_| view! {
                    <span class="text-green-500 ml-2">"Rescan triggered successfully!"</span>
                })}
            </div>
        </div>
    }
}
