use std::ops::Deref;

use crate::Cookies;
use leptos::{html::Ins, prelude::*};
use leptos_router::components::A;
use leptos_use::{use_mutation_observer_with_options, UseMutationObserverOptions};
use log::info;

#[component]
pub fn Ad(#[prop(optional)] class: Option<&'static str>) -> impl IntoView {
    let ad_class = class.unwrap_or("h-64");
    let node = NodeRef::<Ins>::new();
    let cookies = use_context::<Cookies>().unwrap();
    let (hide_ads, _) = cookies.use_cookie_typed::<_, bool>("HIDE_ADS");
    let unfilled = RwSignal::new(false);
    let _mutation_observer = use_mutation_observer_with_options(
        node,
        move |mutations, _| {
            if let Some(_ad_fill_status) = mutations.into_iter().find(|record| {
                record
                    .attribute_name()
                    .map(|name| name == "data-ad-status")
                    .unwrap_or_default()
            }) {
                // just looking for data-ad-status="unfilled"
                let node = node.get_untracked().unwrap();
                if let Some(status) = node.deref().get_attribute("data-ad-status") {
                    info!("ad status {status}");
                    unfilled.set(status == "unfilled");
                }
            }
        },
        UseMutationObserverOptions::default().attributes(true),
    );
    // let location = use_location();
    // let pathname = location.pathname;
    // let search = location.search;

    // let _ = pathname(); // reading from the path to reload this component on page load
    // let _ = search();
    let ads_visible = Signal::derive(move || !hide_ads.get().unwrap_or_default());
    view! {
        <Show when=ads_visible>
            <div class:hidden=unfilled class="ad">
                <div class="flex flex-col h-full">
                    <span class="text-sm p-1 px-2 rounded-md bg-violet-950 shrink max-w-fit">
                        "Ad"
                    </span>
                    <script
                        async
                        src="https://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js?client=ca-pub-8789160460804755"
                        crossorigin="anonymous"
                        on:error=move |_e| unfilled.set(true)
                    ></script>
                    // <!-- Ultros-Ad-Main -->
                    <ins
                        class=["adsbygoogle ", ad_class].concat()
                        style="display:block"
                        data-ad-client="ca-pub-8789160460804755"
                        data-ad-slot="1163555858"
                        // data-adtest="on"
                        node_ref=node
                    ></ins>
                    <script>(adsbygoogle = window.adsbygoogle || []).push({});</script>
                    <span class="text-neutral-500 italic text-sm">
                        "ads support the site. you may disable or enable them under "
                        <A href="/settings">"Settings"</A>
                    </span>
                </div>
            </div>
        </Show>
    }.into_any()
}
