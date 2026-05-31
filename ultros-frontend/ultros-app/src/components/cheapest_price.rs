use leptos::prelude::*;
use xiv_gen::ItemId;

use super::{gil::*, world_name::*};
use crate::{
    components::skeleton::SingleLineSkeleton, global_state::cheapest_prices::CheapestPrices,
    i18n::*,
};
use ultros_api_types::world_helper::AnySelector;

/// Always shows the lowest price.
///
/// Defers reading the `cheapest_prices` resource until after hydration. The
/// `Suspense` fallback (`<SingleLineSkeleton />`) is the only shape that's
/// safe to ship through SSR + first-render CSR for this component: SSR uses
/// `.with()` (not `.read()`), which does NOT subscribe-and-suspend the
/// wrapping Suspense, so the SSR pass renders the fallback with the resource
/// in pending state. The client side serialises the resolved resource into
/// the payload, so the first CSR render of the body would otherwise see
/// `Some(map)` immediately during hydration — swapping the SSR'd
/// `<SingleLineSkeleton />` for either a `<div>` (item has a listing) or
/// nothing (no listing). The shape divergence drives tachys' walker into
/// `failed_to_cast_text_node` at `tachys-0.2.15/src/hydration.rs:227`
/// (the post-debug-strip `unreachable!()` — see the long-running
/// `/items/jobset/<JOB>` and `/item/<world>/<id>` clusters in GlitchTip
/// including issues 4 (count 622), 5277, 2388, 4962, 156, 5002, 224, 5392,
/// 2541, 5030, 1306, 4910, 4889, 5391, 5360, 5359, plus the per-item-page
/// pairs 5413/5414, 5411/5412, 5409/5410, 5407/5408, 5405/5406, 5403/5404,
/// 5401/5402, 5399/5400, 5397/5398, 5395/5396, 5393/5394, 5389/5390,
/// 5387/5388, 5385/5386, 5383/5384 etc.). The wasm-bindgen-futures executor
/// then cascades into `RefCell already borrowed` from the same trace, which
/// is what surfaces as the secondary issue in each pair.
///
/// Same idiom as #730 (relative-time), #732 (source-callout recipe chip),
/// #725 (chart), #719 (item-explorer price sort), #712 (RecentItems): an
/// `Effect`-driven `hydrated` flag (effects run client-only, after the
/// initial view is rendered) so SSR and the first CSR render both render
/// the skeleton, shapes match, and a frame later the effect fires and the
/// closure re-runs with the resolved listings.
#[component]
pub fn CheapestPrice(
    item_id: ItemId,
    #[prop(optional)] show_hq: Option<bool>,
    #[prop(optional, into)] label: Option<String>,
) -> impl IntoView {
    let i18n = use_i18n();
    let cheapest = use_context::<CheapestPrices>().unwrap().read_listings;
    let hydrated = RwSignal::new(false);
    Effect::new(move |_| {
        hydrated.set(true);
    });
    view! {
        <Suspense fallback=move || {
            view! { <SingleLineSkeleton /> }
        }>
            {move || {
                if !hydrated.get() {
                    return view! { <SingleLineSkeleton /> }.into_any();
                }
                cheapest
                    .with(|data| {
                        data.as_ref()
                            .and_then(|data| {
                                data.as_ref()
                                    .ok()
                                    .and_then(|data| {
                                        let listing_data = data.find_matching_listings(item_id.0);
                                        let hq_prefix = t_string!(i18n, cheapest_hq_prefix).to_string();
                                        let hq = listing_data.hq.map(|hq| (hq_prefix.clone(), hq));
                                        let lq = listing_data.lq.map(|lq| (String::new(), lq));
                                        let data = match show_hq {
                                            Some(true) => hq,
                                            Some(false) => lq,
                                            None => hq.or(lq),
                                        };
                                        data.map(|(internal_label, listing)| {
                                            if let Some(label) = label.clone() {
                                                view! {
                                                    <div class="flex flex-col">
                                                         <span class="text-xs text-[color:var(--color-text-muted)] uppercase tracking-wider mb-0.5">{label}</span>
                                                         <div class="flex flex-row items-center gap-1.5">
                                                            <Gil amount=listing.price />
                                                            <span>
                                                                <WorldName id=AnySelector::World(listing.world_id) />
                                                            </span>
                                                        </div>
                                                    </div>
                                                }.into_any()
                                            } else {
                                                view! {
                                                    <div class="flex flex-row items-center gap-1.5">
                                                        {internal_label} <Gil amount=listing.price />
                                                        <span>
                                                            <WorldName id=AnySelector::World(listing.world_id) />
                                                        </span>
                                                    </div>
                                                }.into_any()
                                            }
                                        })
                                    })
                            })
                    })
                    .unwrap_or_else(|| ().into_any())
            }}
        </Suspense>
    }
    .into_any()
}
