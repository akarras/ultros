use crate::components::listing_filters::filter_listing_rows;
use crate::components::listing_quality::{ListingQuality, filter_by_quality};
use crate::components::listings_table::ListingsTable;
use crate::components::skeleton::BoxSkeleton;
use crate::error::AppError;
use crate::global_state::local_world_data::LocalWorldData;
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
    let world_data = use_context::<LocalWorldData>().and_then(|data| data.0.ok());

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
                let quality_rows = Memo::new(move |_| {
                    let all = crate::routes::item_view::get_or_default(&filtered_listings);
                    filter_by_quality(all, quality.get())
                });
                let rows = Memo::new({
                    let world_data = world_data.clone();
                    move |_| {
                        filter_listing_rows(
                            crate::routes::item_view::get_or_default(&quality_rows),
                            world_data.as_deref(),
                            &HashSet::new(),
                            &crate::routes::item_view::get_or_default(&excluded_datacenters),
                        )
                    }
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
                            <span
                                class="text-sm text-[color:var(--color-text-muted)]"
                                data-testid="listings-count"
                            >
                                {move || {
                                    if excluded_datacenters.with(|set| set.is_empty()) {
                                        t_string!(
                                            i18n,
                                            item_view_listings_count,
                                            count = rows.with(|r| r.len()),
                                        )
                                        .to_string()
                                    } else {
                                        t_string!(
                                            i18n,
                                            item_view_filtered_listings_count,
                                            visible = rows.with(|r| r.len()),
                                            total = quality_rows.with(|r| r.len()),
                                        )
                                        .to_string()
                                    }
                                }}
                            </span>
                        </div>
                        <details
                            class="group rounded-lg border border-[color:var(--color-outline)] px-3 py-2"
                            data-testid="datacenter-exclusions"
                        >
                            <summary class="flex min-h-9 cursor-pointer list-none items-center gap-2 rounded-md text-sm font-medium text-brand-300 hover:text-brand-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[color:var(--brand-ring)]">
                                <span>{t!(i18n, item_view_exclude_datacenters)}</span>
                                {move || {
                                    let count = excluded_datacenters.with(|set| set.len());
                                    (count > 0).then(|| {
                                        view! {
                                            <span class="inline-flex min-w-5 items-center justify-center rounded-full bg-[color:var(--brand-bg)] px-1.5 py-0.5 text-xs font-bold text-[color:var(--brand-fg)]">
                                                {count}
                                            </span>
                                        }
                                    })
                                }}
                                <crate::components::icon::Icon
                                    icon=icondata::MdiChevronDown
                                    attr:class="ml-auto transition-transform group-open:rotate-180"
                                />
                            </summary>
                            <div class="mt-2 border-t border-[color:var(--color-outline)] pt-2">
                                <crate::routes::item_view::DatacenterExclusionControls
                                    world=world
                                    excluded_datacenters=excluded_datacenters
                                />
                            </div>
                        </details>
                        {move || {
                            if !rows.with(|rows| rows.is_empty()) {
                                view! { <ListingsTable listings=rows /> }.into_any()
                            } else if !excluded_datacenters.with(|set| set.is_empty())
                                && !quality_rows.with(|rows| rows.is_empty())
                            {
                                view! {
                                    <div
                                        role="status"
                                        class="flex min-h-32 flex-col items-center justify-center gap-3 rounded-lg border border-[color:var(--color-outline)] px-4 py-6 text-center"
                                        data-testid="listings-filter-empty"
                                    >
                                        <p class="text-sm text-[color:var(--color-text-muted)]">
                                            {t!(i18n, item_view_no_listings_match_exclusions)}
                                        </p>
                                        <button
                                            type="button"
                                            class="btn btn-primary"
                                            data-testid="reset-datacenter-exclusions"
                                            on:click=move |_| excluded_datacenters.update(|set| set.clear())
                                        >
                                            {t!(i18n, item_view_reset_exclusions)}
                                        </button>
                                    </div>
                                }
                                    .into_any()
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
