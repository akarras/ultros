use crate::api::{ResaleStatsDto, get_best_deals};
use crate::components::gil::Gil;
use crate::components::icon::Icon;
use crate::components::item_icon::{IconSize, ItemIcon};
use crate::components::skeleton::BoxSkeleton;
use crate::global_state::home_world::use_home_world;
use icondata as i;
use leptos::prelude::*;
use xiv_gen::ItemId;

#[component]
fn DealItem(deal: ResaleStatsDto, home_world_name: String) -> impl IntoView {
    let item = xiv_gen_db::data().items.get(&ItemId(deal.item_id));
    let name = item.map(|i| i.name.as_str()).unwrap_or("Unknown Item");

    view! {
        <a
            href=format!("/item/{}/{}", home_world_name, deal.item_id)
            class="group block p-4 rounded-xl bg-[color:var(--surface-color)] hover:bg-[color:var(--surface-color-hover)] transition-all duration-200 border border-[color:var(--separator-color)] hover:border-[color:var(--brand-ring)] hover:shadow-lg relative overflow-hidden"
        >
            <div class="absolute top-0 right-0 p-2 opacity-10 group-hover:opacity-20 transition-opacity pointer-events-none">
                <Icon icon=i::FaMoneyBillTrendUpSolid width="3em" height="3em" aria_hidden=true />
            </div>
            <div class="flex items-start gap-4 relative z-10">
                <ItemIcon item_id=deal.item_id icon_size=IconSize::Medium />
                <div class="flex-1 min-w-0">
                    <h4 class="font-bold text-[color:var(--color-text)] truncate mb-1">{name}</h4>
                    <div class="flex flex-wrap gap-y-1 gap-x-3 text-sm">
                        <div class="flex items-center gap-1 text-[color:var(--color-text-success)] font-mono font-medium">
                            <span class="text-xs flex items-center">
                                <Icon icon=i::FaArrowTrendUpSolid aria_hidden=true />
                            </span>
                            <Gil amount=deal.profit />
                            <span class="text-xs opacity-80 ml-1">"profit"</span>
                        </div>
                        <div class="flex items-center gap-1 text-[color:var(--color-text-muted)]">
                            <span class="text-xs">"ROI"</span>
                            <span class="font-mono">{format!("{:.0}%", deal.return_on_investment)}</span>
                        </div>
                    </div>
                </div>
            </div>
        </a>
    }
}

#[component]
pub fn TopDeals() -> impl IntoView {
    let (home_world, _) = use_home_world();

    let deals = Resource::new(
        move || home_world.get(),
        move |world| async move {
            if let Some(w) = world {
                get_best_deals(&w.name).await.ok()
            } else {
                None
            }
        },
    );

    view! {
        <div class="panel p-6 rounded-2xl bg-gradient-to-br from-[color:var(--surface-color)] to-[color:var(--surface-color-hover)] relative overflow-hidden">
            <div class="flex items-center justify-between mb-6 relative z-10">
                <div class="flex items-center gap-3">
                    <div class="p-2 rounded-lg bg-[color:var(--brand-bg)] text-[color:var(--brand-fg)]">
                        <Icon icon=i::FaFireSolid width="1.25em" height="1.25em" aria_hidden=true />
                    </div>
                    <div>
                        <h2 class="text-2xl font-extrabold tracking-tight text-[color:var(--color-text)]">
                            "Market Movers"
                        </h2>
                        <p class="text-sm text-[color:var(--color-text-muted)]">
                            "Top flips in your region right now"
                        </p>
                    </div>
                </div>
                <a
                    href="/flip-finder"
                    class="text-sm font-medium text-[color:var(--brand-fg)] hover:text-[color:var(--brand-fg-hover)] hover:underline flex items-center gap-1 transition-colors"
                >
                    "View All"
                    <span class="text-xs flex items-center">
                        <Icon icon=i::FaArrowRightSolid aria_hidden=true />
                    </span>
                </a>
            </div>

            <Suspense fallback=move || view! {
                <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                    <div class="h-24 rounded-xl overflow-hidden"><BoxSkeleton /></div>
                    <div class="h-24 rounded-xl overflow-hidden"><BoxSkeleton /></div>
                    <div class="h-24 rounded-xl overflow-hidden"><BoxSkeleton /></div>
                    <div class="h-24 rounded-xl overflow-hidden"><BoxSkeleton /></div>
                </div>
            }>
                {move || {
                    deals.get().flatten().map(|data| {
                        if data.is_empty() {
                            view! {
                                <div class="text-center py-8 text-[color:var(--color-text-muted)] bg-[color:var(--surface-color-alt)] rounded-xl border border-dashed border-[color:var(--separator-color)]">
                                    <div class="mb-2 opacity-50 mx-auto w-8 h-8 flex items-center justify-center">
                                        <Icon icon=i::FaBoxOpenSolid width="2em" height="2em" aria_hidden=true />
                                    </div>
                                    <p>"No hot deals found right now."</p>
                                    <p class="text-sm">"Check back later or try the full Flip Finder."</p>
                                </div>
                            }.into_any()
                        } else {
                            let world_name = home_world.get().map(|w| w.name).unwrap_or_default();
                            view! {
                                <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                                    <For
                                        each=move || data.clone().into_iter().take(6)
                                        key=|deal| deal.item_id
                                        children=move |deal| {
                                            view! { <DealItem deal=deal home_world_name=world_name.clone() /> }
                                        }
                                    />
                                </div>
                            }.into_any()
                        }
                    })
                }}
            </Suspense>
        </div>
    }
}
