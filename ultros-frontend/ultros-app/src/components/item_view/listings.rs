use crate::api::get_listings;
use crate::components::ad::Ad;
use crate::components::listings_table::ListingsTable;
use crate::components::skeleton::BoxSkeleton;
use crate::error::AppError;
use leptos::prelude::*;
use std::sync::Arc;
use ultros_api_types::CurrentlyShownItem;

use super::chart_wrapper::ChartWrapper;
use super::sales_details::SalesDetails;
use super::summary_cards::SummaryCards;

#[component]
pub fn HighQualityTable(
    listing_resource: Resource<Result<CurrentlyShownItem, AppError>>,
) -> impl IntoView {
    view! {
        <div class="space-y-6">
            <Transition fallback=move || {
                view! { <BoxSkeleton /> }
            }>
                {move || {
                    let hq_listings = Memo::new(move |_| {
                        listing_resource
                            .with(|l| {
                                l.as_ref()
                                    .and_then(|l| {
                                        l.as_ref()
                                            .ok()
                                            .map(|l| {
                                                l.listings
                                                    .iter()
                                                    .filter(|(l, _)| l.hq)
                                                    .map(|(l, r)| (l.clone(), Arc::new(r.clone())))
                                                    .collect::<Vec<_>>()
                                            })
                                    })
                            })
                            .unwrap_or_default()
                    });
                    view! {
                        <div
                            class="panel p-4 sm:p-6"
                            class:hidden=move || hq_listings.with(|l| l.is_empty())
                        >
                            <h2 class="text-xl font-bold text-center mb-4 text-brand-200">
                                "High Quality Listings"
                            </h2>
                            <ListingsTable listings=hq_listings />
                        </div>
                    }
                }}
            </Transition>
        </div>
    }
    .into_any()
}

#[component]
pub fn LowQualityTable(
    listing_resource: Resource<Result<CurrentlyShownItem, AppError>>,
) -> impl IntoView {
    view! {
        <div class="space-y-6">
            <Transition fallback=move || {
                view! { <BoxSkeleton /> }
            }>
                {move || {
                    let lq_listings = Memo::new(move |_| {
                        listing_resource
                            .with(|l| {
                                l.as_ref()
                                    .and_then(|l| {
                                        l.as_ref()
                                            .ok()
                                            .map(|l| {
                                                l.listings
                                                    .iter()
                                                    .filter(|(l, _)| !l.hq)
                                                    .map(|(l, r)| (l.clone(), Arc::new(r.clone())))
                                                    .collect::<Vec<_>>()
                                            })
                                    })
                            })
                            .unwrap_or_default()
                    });
                    view! {
                        <div
                            class="panel p-4 sm:p-6"
                            class:hidden=move || lq_listings.with(|l| l.is_empty())
                        >
                            <h2 class="text-xl font-bold text-center mb-4 text-brand-200">
                                "Low Quality Listings"
                            </h2>
                            <ListingsTable listings=lq_listings />
                        </div>
                    }
                        .into_any()
                }}
            </Transition>
        </div>
    }
    .into_any()
}

#[component]
pub fn ListingsContent(item_id: Memo<i32>, world: Memo<String>) -> impl IntoView {
    let listing_resource = Resource::new(
        move || (item_id(), world()),
        |(item_id, world)| async move {
            get_listings(item_id, world.as_str())
                .await
                .inspect_err(|e| tracing::error!(error = ?e, "Error getting value"))
        },
    );
    Effect::new(move |_| {
        let val = listing_resource.get();
        tracing::info!(?val, "Listings updated");
    });
    view! {
        <div class="w-full py-8 text-[color:var(--color-text)]">
            <SummaryCards listing_resource item_id=item_id() />

            <div id="listings" class="grid grid-cols-1 gap-6">
                <HighQualityTable listing_resource />
                <LowQualityTable listing_resource />
            </div>

            <div id="history" class="grid grid-cols-1 gap-6 mt-8">
                 <ChartWrapper listing_resource item_id world />
                 <SalesDetails listing_resource />
            </div>

            <div class="mt-6 mx-auto">
                <Ad class="h-[336px] w-[280px] rounded-xl overflow-hidden" />
            </div>
        </div>
    }
    .into_any()
}
