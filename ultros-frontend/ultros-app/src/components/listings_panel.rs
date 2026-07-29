use crate::components::listing_quality::{ListingQuality, filter_by_quality};
use crate::components::listings_table::ListingsTable;
use crate::components::skeleton::BoxSkeleton;
use crate::error::AppError;
use crate::i18n::{t, t_string};
use leptos::prelude::*;
use std::collections::HashSet;
use std::sync::Arc;
use ultros_api_types::{ActiveListing, CurrentlyShownItem, Retainer};

type ListingRows = Vec<(ActiveListing, Arc<Retainer>)>;

/// The active-listings section: one table for both qualities, with a filter,
/// rather than two stacked tables with two independent "Show more" buttons.
///
/// The datacenter exclusion controls used to occupy a section of their own;
/// they now live in a `<details>` disclosure in this panel's header.
#[component]
pub fn ListingsPanel(
    listing_resource: Resource<Result<Arc<CurrentlyShownItem>, AppError>>,
    #[prop(into)] filtered_listings: Signal<ListingRows>,
    world: Memo<String>,
    excluded_datacenters: RwSignal<HashSet<String>>,
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
                    <div class="flex flex-col gap-3 rounded-lg border border-[color:var(--color-outline)] p-3 sm:p-4">
                        <div class="flex flex-wrap items-center gap-3">
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
                            <span class="text-sm text-[color:var(--color-text-muted)]">
                                {move || {
                                    t!(i18n, item_view_listings_count, count = rows.with(|r| r.len()))
                                }}
                            </span>
                        </div>
                        <details class="group">
                            <summary class="cursor-pointer text-sm text-brand-300 hover:text-brand-100">
                                {t!(i18n, item_view_exclude_datacenters)}
                            </summary>
                            <div class="mt-2">
                                <crate::routes::item_view::DatacenterExclusionControls
                                    world=world
                                    excluded_datacenters=excluded_datacenters
                                />
                            </div>
                        </details>
                        <ListingsTable listings=rows />
                    </div>
                }
                    .into_any()
            }}
        </Transition>
    }
    .into_any()
}
