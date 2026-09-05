use crate::components::listing_quality::{ListingQuality, filter_by_quality};
use crate::components::listings_table::ListingsTable;
use crate::components::skeleton::BoxSkeleton;
use crate::error::AppError;
use crate::i18n::{t, t_string};
use leptos::prelude::*;
use std::sync::Arc;
use ultros_api_types::{ActiveListing, CurrentlyShownItem, Retainer};

type ListingRows = Vec<(ActiveListing, Arc<Retainer>)>;

/// The active-listings section: one table for both qualities, with a filter,
/// rather than two stacked tables with two independent "Show more" buttons.
#[component]
pub fn ListingsPanel(
    listing_resource: Resource<Result<Arc<CurrentlyShownItem>, AppError>>,
    #[prop(into)] filtered_listings: Signal<ListingRows>,
    item_id: Memo<i32>,
) -> impl IntoView {
    let i18n = crate::i18n::use_i18n();
    let (quality, set_quality) = signal(ListingQuality::default());

    let quality_button = move |value: ListingQuality, label: String| {
        view! {
            <button
                type="button"
                aria-pressed=move || (quality.get() == value).to_string()
                class=move || {
                    [
                        "px-3 py-1 text-sm transition-colors",
                        if quality.get() == value {
                            "bg-[color:var(--brand-bg)] text-[color:var(--brand-fg)] font-bold"
                        } else {
                            "text-[color:var(--color-text-muted)] hover:text-brand-100"
                        },
                    ]
                        .join(" ")
                }
                on:click=move |_| set_quality.set(value)
            >
                {label}
            </button>
        }
    };

    view! {
        <Transition fallback=move || view! { <BoxSkeleton /> }>
            {move || {
                // Read `listing_resource` inside the Transition so this section
                // actually suspends on it during SSR. `filtered_listings` is a Memo
                // created outside any Suspense boundary, so reading it alone does NOT
                // subscribe this Transition to the resource — the server would then
                // render an empty table while the client hydrates a populated one,
                // tripping the tachys hydration `unreachable!()` panic (GlitchTip #6831).
                if !listing_resource.with(|r| matches!(r, Some(Ok(_)))) {
                    return ().into_any();
                }
                let rows = Memo::new(move |_| {
                    let all = crate::routes::item_view::get_or_default(&filtered_listings);
                    filter_by_quality(all, quality.get())
                });
                view! {
                    <div class="flex h-full flex-col gap-3 rounded-lg border border-[color:var(--color-outline)] p-3 sm:p-4">
                        <div class="flex min-h-8 flex-wrap items-center gap-3">
                            <h2 class="text-xl font-bold text-brand-200">
                                {t!(i18n, active_listings)}
                            </h2>
                            <div
                                role="group"
                                aria-label=move || {
                                    t_string!(i18n, item_view_quality_filter_aria).to_string()
                                }
                                class="inline-flex overflow-hidden rounded-md border border-[color:var(--color-outline)]"
                            >
                                {quality_button(
                                    ListingQuality::All,
                                    t_string!(i18n, item_view_quality_all).to_string(),
                                )}
                                {quality_button(ListingQuality::Hq, t_string!(i18n, hq).to_string())}
                                {quality_button(ListingQuality::Nq, t_string!(i18n, nq).to_string())}
                            </div>
                            <span
                                class="text-sm text-[color:var(--color-text-muted)]"
                                data-testid="listings-count"
                            >
                                {move || t!(i18n, item_view_listings_count, count = rows.with(|rows| rows.len()))}
                            </span>
                        </div>
                        <div class="flex min-h-6 flex-wrap items-center gap-x-4 gap-y-1 text-sm text-[color:var(--color-text-muted)]" data-testid="listings-summary">
                            <span>{t!(i18n, cheapest_found)}</span>
                            {move || {
                                [false, true].into_iter().filter_map(|hq| {
                                    rows.with(|rows| crate::routes::item_view::cheapest_listing_for_quality(rows, hq)).map(|(listing, _)| {
                                        view! {
                                            <div class="flex items-center gap-1.5">
                                                <span class="font-semibold">{if hq { t!(i18n, hq).into_any() } else { t!(i18n, nq).into_any() }}</span>
                                                <crate::components::gil::Gil amount=listing.price_per_unit />
                                                <crate::components::world_name::WorldName id=ultros_api_types::world_helper::AnySelector::World(listing.world_id) />
                                            </div>
                                        }
                                    })
                                }).collect_view()
                            }}
                            {move || rows.with(|rows| rows.is_empty()).then(|| view! { <span>{t!(i18n, no_data)}</span> })}
                            <crate::routes::item_view::RealPriceSummary listing_resource item_id />
                        </div>
                        {move || {
                            if !rows.with(|rows| rows.is_empty()) {
                                view! { <div><ListingsTable listings=rows /></div> }.into_any()
                            } else {
                                view! {
                                    <div
                                        role="status"
                                        class="flex min-h-32 items-center justify-center rounded-lg border border-[color:var(--color-outline)] px-4 py-6 text-center text-sm text-[color:var(--color-text-muted)]"
                                    >
                                        {t!(i18n, no_active_listings_found)}
                                    </div>
                                }
                                    .into_any()
                            }
                        }}
                    </div>
                }
                    .into_any()
            }}
        </Transition>
    }
    .into_any()
}
