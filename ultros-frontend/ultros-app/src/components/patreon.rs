use leptos::prelude::*;

#[component]
pub fn PatreonWrapper(children: Children) -> impl IntoView {
    let show = RwSignal::new(false);
    view! {
        <div
            class="flex flex-col"
            on:click=move |_| {
                show.update(|show| {
                    *show = !*show;
                });
            }
        >
            <div
                class="fixed bottom-0 left-0 flex flex-col cursor-not-allowed"
                class:invisible=move || !show()
            >
                <img src=move || {
                    if show() { "/static/images/leekspin.gif".to_string() } else { String::new() }
                } />
                <Show when=show>
                    <audio
                        controls
                        loop
                        autoplay
                        src=move || {
                            if show() { "/static/ratata.mp3".to_string() } else { String::new() }
                        }
                    />
                </Show>
            </div>
            {children()}
        </div>
    }
}

