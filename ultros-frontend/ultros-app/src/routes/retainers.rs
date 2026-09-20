use crate::api::{
    UndercutData, get_login, get_retainer_listings, get_retainer_undercuts,
    get_user_retainer_listings,
};
use crate::components::alert_drawer::{AlertDrawer, AlertKind};
use crate::components::app_link::AppLink;
use crate::components::clipboard::Clipboard;
use crate::components::data_table::{
    Column, ColumnHeader, TrackWidths, body_cells, header_cells, visible_column_count,
};
use crate::components::gil::*;
use crate::components::icon::Icon;
use crate::components::realtime_status::RealtimeStatus;
use crate::components::skeleton::{BoxSkeleton, SkeletonCell, SkeletonColumn, TableSkeleton};
use crate::components::tool_help::{ActionableEmptyState, ToolHeader};
use crate::components::{item_icon::*, meta::*, world_name::*};
use crate::global_state::use_world_display_name;
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;
use crate::routes::retainer_live::{ListedPair, use_retainer_live};
use components::Outlet;
use hooks::use_params_map;
use icondata as i;
use leptos::either::Either;
use leptos::prelude::*;
use leptos_i18n::I18nContext;
use leptos_router::*;
use std::sync::Arc;
use ultros_api_types::{ActiveListing, FfxivCharacter, Retainer, world_helper::AnySelector};
use xiv_gen::{ItemId, ItemSortCategoryId};

/// Skeleton columns for [`RetainerTable`]: HQ flag, item, price, quantity,
/// total — the same five columns the real `<table>` renders, in the same
/// order, so the loading state has the real table's rhythm.
fn listing_skeleton_columns() -> Vec<SkeletonColumn> {
    vec![
        SkeletonColumn::new("w-10 px-3 py-2", SkeletonCell::Blank),
        SkeletonColumn::new("flex-1 min-w-40 px-3 py-2", SkeletonCell::IconText),
        SkeletonColumn::new("w-24 px-3 py-2", SkeletonCell::Number),
        SkeletonColumn::new("w-16 px-3 py-2", SkeletonCell::Number),
        SkeletonColumn::new("w-24 px-3 py-2", SkeletonCell::Number),
    ]
}

/// Skeleton columns for [`RetainerUndercutTable`]: [`listing_skeleton_columns`]
/// plus the "undercut by one" column that table adds.
fn undercut_skeleton_columns() -> Vec<SkeletonColumn> {
    let mut cols = listing_skeleton_columns();
    cols.push(SkeletonColumn::new("w-28 px-3 py-2", SkeletonCell::Number));
    cols
}

/// Shared header-cell classes for both retainer tables' `<table>` substrate —
/// see the substrate note in `components/data_table.rs` for why these tables
/// keep a real `<table>` rather than moving to `DataTableGrid`: their columns
/// are content-sized and there is no responsive column set to express.
const TH: &str = "px-3 py-2 font-bold whitespace-nowrap text-left";

#[derive(PartialOrd, Ord, Eq, PartialEq, Debug)]
struct ItemSortKey(u8, i32, i32, bool, i32);

impl From<(ItemId, bool)> for ItemSortKey {
    fn from((item_id, hq): (ItemId, bool)) -> Self {
        let inner = move || {
            let data = tracked_data();
            let items = &data.items;
            let sort_category = &data.item_sort_categorys;
            let item = items.get(&item_id)?;
            let sort_weight = sort_category
                .get(&ItemSortCategoryId(item.item_sort_category as i32))
                .map(|category| category.param)?;
            Some(Self(
                sort_weight as u8,
                item.subcategory_sort,
                -item.level_item,
                !hq,
                item.key_id.0,
            ))
        };
        inner().unwrap_or(Self(u8::MAX, i32::MAX, i32::MAX, hq, i32::MAX))
    }
}

impl From<&ActiveListing> for ItemSortKey {
    fn from(listing: &ActiveListing) -> Self {
        ItemSortKey::from((ItemId(listing.item_id), listing.hq))
    }
}

/// One column, shared between [`RetainerTable`] and [`RetainerUndercutTable`]
/// via a common row shape both listing kinds can produce — the point being
/// the header, the body rows and the empty state's `colspan` can no longer
/// disagree about which columns exist or what order they come in (the debt
/// #1080 retired for the item explorer and the currency exchange).
///
/// The row is also its own `<For>` key: every field a cell renders is in
/// here, so a refetch re-creates exactly the rows whose listing changed and
/// leaves every other `<tr>` untouched.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct RetainerRow {
    /// The listing's own id, so two otherwise identical listings on one
    /// retainer stay two rows.
    listing_id: i32,
    hq: bool,
    item_id: i32,
    price_per_unit: i32,
    quantity: i32,
    /// `Some` only on the undercuts table: the price one gil under the
    /// current cheapest listing.
    undercut_by_one: Option<i32>,
}

impl From<&ActiveListing> for RetainerRow {
    fn from(listing: &ActiveListing) -> Self {
        Self {
            listing_id: listing.id,
            hq: listing.hq,
            item_id: listing.item_id,
            price_per_unit: listing.price_per_unit,
            quantity: listing.quantity,
            undercut_by_one: None,
        }
    }
}

impl From<&UndercutData> for RetainerRow {
    fn from(undercut_data: &UndercutData) -> Self {
        let listing = &undercut_data.current;
        Self {
            listing_id: listing.id,
            hq: listing.hq,
            item_id: listing.item_id,
            price_per_unit: listing.price_per_unit,
            quantity: listing.quantity,
            undercut_by_one: Some(undercut_data.cheapest - 1),
        }
    }
}

/// The rows of a [`RetainerTable`], in display order.
fn listing_rows(listings: &[ActiveListing]) -> Vec<RetainerRow> {
    let mut listings: Vec<&ActiveListing> = listings.iter().collect();
    listings.sort_by_key(|listing| ItemSortKey::from(*listing));
    listings.into_iter().map(RetainerRow::from).collect()
}

/// The rows of a [`RetainerUndercutTable`], in display order.
fn undercut_rows(undercuts: &[UndercutData]) -> Vec<RetainerRow> {
    let mut undercuts: Vec<&UndercutData> = undercuts.iter().collect();
    undercuts.sort_by_key(|undercut| ItemSortKey::from(&undercut.current));
    undercuts.into_iter().map(RetainerRow::from).collect()
}

/// The five columns every retainer listing table shows, in DOM order. The
/// item column needs the retainer's world name to link into `/item/{world}/…`,
/// so it takes that as `world_name`. Both tables' `<table>` substrate keeps a
/// real `<table>` rather than moving to `DataTableGrid`'s div grid — the
/// columns are sized to their content and there is no responsive column set
/// to express, so `TrackWidths` goes unused here (see the substrate note in
/// `components/data_table.rs`).
fn base_retainer_columns(
    i18n: I18nContext<Locale, I18nKeys>,
    world_name: Arc<str>,
) -> Vec<Column<RetainerRow>> {
    vec![
        Column::new(
            TrackWidths::default(),
            ColumnHeader::content(move || view! { {t!(i18n, retainers_hq)} }.into_any()),
            move |row: &RetainerRow| {
                view! { <td class="px-3 py-2">{row.hq.then_some(t!(i18n, retainers_hq))}</td> }
                    .into_any()
            },
        )
        .header_class(TH),
        Column::new(
            TrackWidths::default(),
            ColumnHeader::content(move || view! { {t!(i18n, retainers_item)} }.into_any()),
            move |row: &RetainerRow| {
                let item = tracked_data().items.get(&ItemId(row.item_id));
                let item_id = row.item_id;
                let world_name = world_name.clone();
                view! {
                    <td class="px-3 py-2">
                        {if let Some(item) = item {
                            Either::Left(
                                view! {
                                    <div class="flex flex-row items-center gap-1">
                                        <AppLink
                                            attr:class="flex flex-row items-center gap-1"
                                            href=format!("/item/{world_name}/{item_id}")
                                        >
                                            <ItemIcon icon_size=IconSize::Small item_id=item_id />
                                            {item.name.as_str()}
                                        </AppLink>
                                        <Clipboard clipboard_text=item.name.as_str() />
                                    </div>
                                },
                            )
                        } else {
                            Either::Right(view! { {t!(i18n, retainers_item_not_found)} })
                        }}
                    </td>
                }
                .into_any()
            },
        )
        .header_class(TH),
        Column::new(
            TrackWidths::default(),
            ColumnHeader::content(move || {
                view! { {t!(i18n, retainers_price_per_unit)} }.into_any()
            }),
            |row: &RetainerRow| {
                view! {
                    <td class="px-3 py-2 text-right tabular-nums">
                        <Gil amount=row.price_per_unit />
                    </td>
                }
                .into_any()
            },
        )
        .header_class(TH),
        Column::new(
            TrackWidths::default(),
            ColumnHeader::content(move || view! { {t!(i18n, retainers_quantity)} }.into_any()),
            |row: &RetainerRow| {
                view! { <td class="px-3 py-2 text-right tabular-nums">{row.quantity}</td> }
                    .into_any()
            },
        )
        .header_class(TH),
        Column::new(
            TrackWidths::default(),
            ColumnHeader::content(move || view! { {t!(i18n, retainers_total)} }.into_any()),
            |row: &RetainerRow| {
                let total = row.quantity * row.price_per_unit;
                view! {
                    <td class="px-3 py-2 text-right tabular-nums">
                        <Gil amount=total />
                    </td>
                }
                .into_any()
            },
        )
        .header_class(TH),
    ]
}

/// One panel holding a `<table>` for `rows`, using `columns` for the header,
/// the body and the empty state's `colspan`. Shared by [`RetainerTable`] and
/// [`RetainerUndercutTable`] so the panel/table chrome can't drift between
/// them.
///
/// The body is a keyed `<For>` over `rows`: when the page refetches after a
/// listing event, only the rows whose [`RetainerRow`] changed are re-created.
fn retainer_table_panel(
    retainer_name: String,
    retainer_id: i32,
    world_id: i32,
    columns: Vec<Column<RetainerRow>>,
    rows: Signal<Vec<RetainerRow>>,
    empty_message: impl Fn() -> AnyView + Send + Sync + 'static,
) -> impl IntoView {
    let columns = Arc::new(columns);
    let column_count = visible_column_count(&columns).to_string();
    let is_empty = Memo::new(move |_| rows.with(|rows| rows.is_empty()));
    view! {
        <div id=format!("retainer-{retainer_id}") class="panel p-4 rounded-xl scroll-mt-20">
            <span class="content-title">
                {retainer_name} " - " <WorldName id=AnySelector::World(world_id) />
            </span>
            <div class="overflow-x-auto">
                <table class="w-full text-sm text-left">
                    <thead class="text-xs font-bold uppercase tracking-wider text-[color:var(--color-text-muted)]">
                        <tr class="border-b border-white/5">{header_cells(&columns)}</tr>
                    </thead>
                    <tbody class="divide-y divide-white/5">
                        <Show when=move || is_empty.get()>
                            <tr>
                                <td colspan=column_count.clone() class="p-4 text-center opacity-70">
                                    {empty_message()}
                                </td>
                            </tr>
                        </Show>
                        <For
                            each=move || rows.get()
                            key=|row| *row
                            children=move |row| {
                                view! {
                                    <tr class="hover:bg-white/5 transition-colors">
                                        {body_cells(&columns, &row)}
                                    </tr>
                                }
                            }
                        />
                    </tbody>
                </table>
            </div>
        </div>
    }
}

#[component]
fn RetainerUndercutTable(retainer: Retainer, listings: Signal<Vec<UndercutData>>) -> impl IntoView {
    let i18n = use_i18n();
    let world_name: Arc<str> = use_world_display_name(AnySelector::World(retainer.world_id))
        .unwrap_or_default()
        .into();

    let mut columns = base_retainer_columns(i18n, world_name);
    columns.push(
        Column::new(
            TrackWidths::default(),
            ColumnHeader::content(move || {
                view! { {t!(i18n, retainers_undercut_by_one)} }.into_any()
            }),
            |row: &RetainerRow| {
                let new_best_price = row.undercut_by_one.unwrap_or_default();
                view! {
                    <td class="px-3 py-2">
                        <div class="flex flex-row items-center gap-1 justify-end">
                            <Gil amount=new_best_price />
                            <Clipboard clipboard_text=new_best_price.to_string() />
                        </div>
                    </td>
                }
                .into_any()
            },
        )
        .header_class(TH),
    );
    let rows = Memo::new(move |_| listings.with(|listings| undercut_rows(listings)));

    retainer_table_panel(
        retainer.name.clone(),
        retainer.id,
        retainer.world_id,
        columns,
        rows.into(),
        move || view! { {t!(i18n, retainers_undercuts_empty)} }.into_any(),
    )
}

#[component]
fn RetainerTable(retainer: Retainer, listings: Signal<Vec<ActiveListing>>) -> impl IntoView {
    let i18n = use_i18n();
    let world_name: Arc<str> = use_world_display_name(AnySelector::World(retainer.world_id))
        .unwrap_or_default()
        .into();

    let columns = base_retainer_columns(i18n, world_name);
    let rows = Memo::new(move |_| listings.with(|listings| listing_rows(listings)));

    retainer_table_panel(
        retainer.name.clone(),
        retainer.id,
        retainer.world_id,
        columns,
        rows.into(),
        move || view! { {t!(i18n, retainers_listings_empty)} }.into_any(),
    )
}

/// The user's retainers grouped by character, as both retainer pages'
/// resources deliver them: `T` is the per-listing payload each page shows.
type RetainerGroups<T> = Vec<(Option<FfxivCharacter>, Vec<(Retainer, Vec<T>)>)>;

/// Every character heading and retainer panel in `groups`, keyed at each
/// level so a refetch that changes one listing touches one row: characters
/// and retainers keep their DOM nodes, `table` runs only for a retainer new
/// to the list, and the listings it is handed narrow `groups` down to that
/// one retainer.
fn retainer_groups_view<T>(
    groups: Memo<RetainerGroups<T>>,
    table: impl Fn(Retainer, Signal<Vec<T>>) -> AnyView + Send + Sync + Clone + 'static,
) -> impl IntoView
where
    T: Clone + PartialEq + Send + Sync + 'static,
{
    view! {
        <For
            each=move || {
                groups
                    .with(|groups| {
                        groups.iter().map(|(character, _)| character.clone()).collect::<Vec<_>>()
                    })
            }
            key=|character| {
                character
                    .as_ref()
                    .map(|character| {
                        (character.id, character.first_name.clone(), character.last_name.clone())
                    })
            }
            children=move |character| {
                let table = table.clone();
                let group = character.clone();
                let retainers = Memo::new(move |_| {
                    groups
                        .with(|groups| {
                            groups
                                .iter()
                                .find(|(character, _)| *character == group)
                                .map(|(_, retainers)| {
                                    retainers
                                        .iter()
                                        .map(|(retainer, _)| retainer.clone())
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_default()
                        })
                });
                let group = character.clone();
                view! {
                    <div class="flex flex-col gap-2">
                        {character
                            .map(|character| {
                                view! {
                                    <span class="content-title font-semibold mt-2">
                                        {character.first_name} " " {character.last_name}
                                    </span>
                                }
                            })}
                        <For
                            each=move || retainers.get()
                            key=|retainer| (retainer.id, retainer.name.clone(), retainer.world_id)
                            children=move |retainer| {
                                let group = group.clone();
                                let retainer_id = retainer.id;
                                let listings = Memo::new(move |_| {
                                    groups
                                        .with(|groups| {
                                            groups
                                                .iter()
                                                .find(|(character, _)| *character == group)
                                                .and_then(|(_, retainers)| {
                                                    retainers
                                                        .iter()
                                                        .find(|(retainer, _)| retainer.id == retainer_id)
                                                })
                                                .map(|(_, listings)| listings.clone())
                                                .unwrap_or_default()
                                        })
                                });
                                table(retainer, listings.into())
                            }
                        />
                    </div>
                }
            }
        />
    }
}

/// The `(world, item)` pairs a retainer page's live subscription watches, as
/// a sorted set. Both pages feed this to [`use_retainer_live`] through a
/// `Memo`, so a live refetch that returns the same items in a different
/// order — or the same listings with new prices — does not re-run the
/// subscription effect and tear the websocket subscription down to rebuild
/// it identically (with the status flipping back to "connecting").
fn watched_pairs(pairs: impl IntoIterator<Item = ListedPair>) -> Vec<ListedPair> {
    let mut pairs: Vec<ListedPair> = pairs.into_iter().collect();
    pairs.sort_unstable();
    pairs.dedup();
    pairs
}

/// Where a retainer page's resource is, with the payload left out so a
/// refetch that only changes listings does not change the phase — the
/// loaded view is gated on this and so survives the refetch intact.
///
/// The boundary around it must be a `<Transition>`, not a `<Suspense>`: a
/// refetch re-registers the resource as pending with whichever boundary
/// read it, and `<Suspense>` answers that by swapping the whole loaded tree
/// out for the fallback skeleton and back — every table on the page
/// vanishing and reappearing on each live update, however carefully the
/// rows underneath are keyed. `<Transition>` keeps the loaded children
/// mounted after the first load and lets the keyed `<For>`s do the diff.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Phase {
    Loading,
    Loaded,
    Failed(String),
}

impl Phase {
    fn of<T>(value: Option<Result<T, crate::error::AppError>>) -> Self {
        match value {
            None => Self::Loading,
            Some(Ok(_)) => Self::Loaded,
            Some(Err(e)) => Self::Failed(e.to_string()),
        }
    }
}

#[component]
pub fn RetainerUndercuts() -> impl IntoView {
    let i18n = use_i18n();
    let login = Resource::new(|| (), |_| async move { get_login().await });
    let retainers = Resource::new(
        move || login.get().map(|res| res.is_ok()).unwrap_or(false),
        move |logged_in| async move {
            if logged_in {
                get_retainer_undercuts().await
            } else {
                Err(crate::error::AppError::ApiError(
                    ultros_api_types::result::ApiError::NotAuthenticated,
                ))
            }
        },
    );
    // Watch everything the retainers list, not just the rows currently
    // undercut — the whole point is catching the moment a cheapest listing
    // stops being cheapest.
    let listed_pairs = Memo::new(move |_| {
        retainers
            .get()
            .and_then(|result| result.ok().map(|report| watched_pairs(report.listed)))
    });
    let live = use_retainer_live(listed_pairs.into(), move || retainers.refetch());
    let phase = Memo::new(move |_| Phase::of(retainers.get()));
    let groups = Memo::new(move |_| {
        retainers
            .get()
            .and_then(|result| result.ok().map(|report| report.undercuts))
            .unwrap_or_default()
    });
    let (drawer_visible, set_drawer_visible) = signal(false);
    view! {
        <MetaTitle title=t_string!(i18n, retainers_undercuts_title).to_string() />
        <Transition fallback=move || {
            view! { <TableSkeleton columns=undercut_skeleton_columns() rows=5 /> }
        }>
            {move || {
                match login.get() {
                    None => {
                        view! { <TableSkeleton columns=undercut_skeleton_columns() rows=5 /> }
                            .into_any()
                    }
                    Some(Err(_)) => {
                        view! {
                            <ActionableEmptyState
                                title=t_string!(i18n, retainers_empty_title).to_string()
                                body=t_string!(i18n, retainers_empty_body).to_string()
                                action_href="/login?next=/retainers/undercuts"
                                action_label=t_string!(i18n, sign_in_discord).to_string()
                                action_external=true
                                secondary_action_href="/bot"
                                secondary_action_label=t_string!(i18n, retainers_empty_secondary_label).to_string()
                            />
                        }.into_any()
                    }
                    Some(Ok(_)) => {
                        view! {
                            <div class="flex flex-wrap items-center justify-between gap-3">
                                <span class="content-title">{t!(i18n, retainers_undercuts_title)}</span>
                                <div class="flex items-center gap-3">
                                    <RealtimeStatus status=live.status last_update=live.last_update />
                                    <button class="btn" on:click=move |_| set_drawer_visible.set(true)>
                                        <Icon icon=i::BsBell />
                                        <span class="ml-1">{t!(i18n, add_alert_button)}</span>
                                    </button>
                                </div>
                            </div>
                            <Show when=move || drawer_visible.get()>
                                <AlertDrawer
                                    initial_kind=AlertKind::Undercut
                                    set_visible=set_drawer_visible.into()
                                />
                            </Show>
                            <br />
                            <span>
                                {t!(i18n, retainers_undercuts_description)}
                            </span>
                            // Registers the resource with the boundary so SSR
                            // awaits it; the `phase` memo below reads it under
                            // its own owner and would not. Returns a real
                            // element: an erased closure returning `()` leaves an
                            // extra SSR placeholder the client never consumes
                            // during hydration (#1280). `hidden` keeps it out of
                            // the layout.
                            {move || {
                                let _ = retainers.get();
                                view! { <span hidden aria-hidden="true"></span> }
                            }}
                            {move || {
                                match phase.get() {
                                    Phase::Loading => {
                                        view! {
                                            <TableSkeleton columns=undercut_skeleton_columns() rows=5 />
                                        }
                                            .into_any()
                                    }
                                    Phase::Loaded => {
                                        view! {
                                            <div>
                                                {retainer_groups_view(
                                                    groups,
                                                    |retainer, listings| {
                                                        view! { <RetainerUndercutTable retainer listings /> }
                                                            .into_any()
                                                    },
                                                )}
                                            </div>
                                        }
                                            .into_any()
                                    }
                                    Phase::Failed(e) => {
                                        view! {
                                            <div>{t!(i18n, retainers_unable_to_get)} <br /> {e}</div>
                                        }
                                            .into_any()
                                    }
                                }
                            }}
                        }.into_any()
                    }
                }
            }}
        </Transition>
    }
}

#[component]
pub fn RetainersBasePath() -> impl IntoView {
    let i18n = use_i18n();
    let login = Resource::new(|| (), |_| async move { get_login().await });
    view! {
        <Suspense fallback=move || view! { <BoxSkeleton rows=2 /> }>
            {move || match login.get() {
                None => view! { <BoxSkeleton rows=2 /> }.into_any(),
                Some(Err(_)) => {
                    view! {
                        <ActionableEmptyState
                            title=t_string!(i18n, retainers_empty_title).to_string()
                            body=t_string!(i18n, retainers_empty_body).to_string()
                            action_href="/login?next=/retainers"
                            action_label=t_string!(i18n, sign_in_discord).to_string()
                            action_external=true
                            secondary_action_href="/bot"
                            secondary_action_label=t_string!(i18n, retainers_empty_secondary_label).to_string()
                        />
                    }.into_any()
                }
                Some(Ok(_)) => {
                    view! {
                        // The tab shell's `ToolHeader` already carries the
                        // page's `<h1>`, so this landing pane is just the
                        // orientation text for whoever hasn't picked a tab
                        // yet.
                        <p class="text-sm text-[color:var(--color-text-muted)]">
                            {t!(i18n, retainers_base_path_description)}
                        </p>
                    }.into_any()
                }
            }}
        </Suspense>
    }
}

#[component]
pub fn SingleRetainerListings() -> impl IntoView {
    let i18n = use_i18n();
    let params = use_params_map();
    let retainer_listings = Resource::new(
        move || params().get("id").and_then(|id| id.parse::<i32>().ok()),
        move |id| async move {
            if let Some(id) = id {
                Some(get_retainer_listings(id).await)
            } else {
                None
            }
        },
    );

    view! {
        <span>
            {t!(i18n, retainers_claim_prompt_start)}
            <AppLink href="/retainers/edit">{t!(i18n, retainers_claim_prompt_link)}</AppLink>
        </span>
        <Suspense fallback=move || {
            view! {
                <div class="panel p-4 rounded-xl">
                    <TableSkeleton columns=listing_skeleton_columns() rows=6 />
                </div>
            }
        }>
            {move || {
                retainer_listings
                    .get()
                    .map(|r| {
                        r.and_then(|r| {
                            r.ok()
                                .map(|r| {
                                    let world_name = use_world_display_name(
                                            AnySelector::World(r.retainer.world_id),
                                        )
                                        .unwrap_or_default();
                                    view! {
                                        <MetaTitle title=format!(
                                            "{} - 🌍{}",
                                            &r.retainer.name,
                                            world_name,
                                        ) />
                                        <MetaDescription text=format!(
                                            "All of the listings for the retainer {} on the world {}",
                                            &r.retainer.name,
                                            world_name,
                                        ) />
                                        <RetainerTable
                                            retainer=r.retainer
                                            listings=Signal::stored(r.listings)
                                        />
                                    }
                                })
                        })
                    })
            }}

        </Suspense>
    }
}

#[component]
pub fn RetainerListings() -> impl IntoView {
    let i18n = use_i18n();
    let login = Resource::new(|| (), |_| async move { get_login().await });
    let retainers = Resource::new(
        move || login.get().map(|res| res.is_ok()).unwrap_or(false),
        move |logged_in| async move {
            if logged_in {
                get_user_retainer_listings().await
            } else {
                Err(crate::error::AppError::ApiError(
                    ultros_api_types::result::ApiError::NotAuthenticated,
                ))
            }
        },
    );
    let (drawer_visible, set_drawer_visible) = signal(false);
    let listed_pairs = Memo::new(move |_| {
        retainers.get().and_then(|result| {
            result.ok().map(|data| {
                watched_pairs(
                    data.retainers
                        .iter()
                        .flat_map(|(_, retainers)| retainers.iter())
                        .flat_map(|(_, listings)| listings.iter())
                        .map(|listing| (listing.world_id, listing.item_id)),
                )
            })
        })
    });
    let live = use_retainer_live(listed_pairs.into(), move || retainers.refetch());
    let phase = Memo::new(move |_| Phase::of(retainers.get()));
    let groups = Memo::new(move |_| {
        retainers
            .get()
            .and_then(|result| result.ok().map(|data| data.retainers))
            .unwrap_or_default()
    });
    let has_none = Memo::new(move |_| groups.with(|groups| groups.is_empty()));
    view! {
        <MetaTitle title=t_string!(i18n, retainers_all_listings_title).to_string() />
        <MetaDescription text=t_string!(i18n, retainers_all_listings_desc).to_string() />
        <Transition fallback=move || {
            view! { <TableSkeleton columns=listing_skeleton_columns() rows=5 /> }
        }>
            {move || {
                match login.get() {
                    None => {
                        view! { <TableSkeleton columns=listing_skeleton_columns() rows=5 /> }
                            .into_any()
                    }
                    Some(Err(_)) => {
                        view! {
                            <ActionableEmptyState
                                title=t_string!(i18n, retainers_empty_title).to_string()
                                body=t_string!(i18n, retainers_empty_body).to_string()
                                action_href="/login?next=/retainers/listings"
                                action_label=t_string!(i18n, sign_in_discord).to_string()
                                action_external=true
                                secondary_action_href="/bot"
                                secondary_action_label=t_string!(i18n, retainers_empty_secondary_label).to_string()
                            />
                        }.into_any()
                    }
                    Some(Ok(_)) => {
                        view! {
                            // Title row lives inside the logged-in arm (like
                            // the undercuts page) so the live pill never
                            // shows a perpetual "connecting" to a visitor who
                            // has nothing to subscribe to.
                            <div class="flex flex-wrap items-center justify-between gap-3 mb-3">
                                <span class="content-title">{t!(i18n, retainers_all_listings_title)}</span>
                                <div class="flex items-center gap-2">
                                    <RealtimeStatus status=live.status last_update=live.last_update />
                                    // Same arm as the pill: a sale alert needs
                                    // retainers to watch, so it is not offered
                                    // to a visitor who has none.
                                    <button
                                        class="btn"
                                        on:click=move |_| set_drawer_visible.set(true)
                                    >
                                        <Icon icon=i::BsBell />
                                        <span class="ml-1">{t!(i18n, add_alert_button)}</span>
                                    </button>
                                </div>
                            </div>
                            <Show when=move || drawer_visible.get()>
                                <AlertDrawer
                                    initial_kind=AlertKind::Sold
                                    set_visible=set_drawer_visible.into()
                                />
                            </Show>
                            // Registers the resource with the boundary so SSR
                            // awaits it; the `phase` memo below reads it under
                            // its own owner and would not. Returns a real
                            // element: an erased closure returning `()` leaves an
                            // extra SSR placeholder the client never consumes
                            // during hydration (#1280). `hidden` keeps it out of
                            // the layout.
                            {move || {
                                let _ = retainers.get();
                                view! { <span hidden aria-hidden="true"></span> }
                            }}
                            {move || {
                                match phase.get() {
                                    Phase::Loading => {
                                        view! {
                                            <TableSkeleton columns=listing_skeleton_columns() rows=5 />
                                        }
                                            .into_any()
                                    }
                                    Phase::Loaded => {
                                        view! {
                                            <Show when=move || has_none.get()>
                                                <span>{t!(i18n, retainers_add_to_start)}</span>
                                            </Show>
                                            <div>
                                                {retainer_groups_view(
                                                    groups,
                                                    |retainer, listings| {
                                                        view! { <RetainerTable retainer listings /> }.into_any()
                                                    },
                                                )}
                                            </div>
                                        }
                                            .into_any()
                                    }
                                    Phase::Failed(e) => {
                                        view! {
                                            <div>{t!(i18n, retainers_unable_to_get)} <br /> {e}</div>
                                        }
                                            .into_any()
                                    }
                                }
                            }}
                        }.into_any()
                    }
                }
            }}
        </Transition>
    }.into_any()
}

#[component]
pub fn Retainers() -> impl IntoView {
    let i18n = use_i18n();
    view! {
        <MetaRobotsNoIndex />
        <div class="main-content p-2 sm:p-6">
            // The three sub-routes below (Edit / All Listings / Undercuts) had
            // no page `<h1>` at all before this rebuild — this `ToolHeader`,
            // shared by the tab shell rather than repeated per tab, is it.
            <ToolHeader
                title=t_string!(i18n, retainers_title).to_string()
                summary=t_string!(i18n, retainers_tool_summary).to_string()
                context=t_string!(i18n, retainers_tool_context).to_string()
                help_href="/help"
                help_body=t_string!(i18n, retainers_tool_help).to_string()
            />
            <div class="flex items-center gap-2 md:gap-3 my-3">
                <AppLink exact=true attr:class="nav-link" href="/retainers/edit">
                    <Icon height="1.25em" width="1.25em" icon=i::BsPencilFill />
                    <span>{t!(i18n, retainers_edit_tab)}</span>
                </AppLink>
                <AppLink exact=true attr:class="nav-link" href="/retainers/listings">
                    <Icon height="1.25em" width="1.25em" icon=i::AiOrderedListOutlined />
                    <span>{t!(i18n, retainers_all_listings_tab)}</span>
                </AppLink>
                <AppLink exact=true attr:class="nav-link" href="/retainers/undercuts">
                    <Icon height="1.25em" width="1.25em" icon=i::AiExclamationOutlined />
                    <span>{t!(i18n, retainers_undercuts_tab)}</span>
                </AppLink>
            </div>
            <Outlet />
        </div>
    }
    .into_any()
}

#[cfg(test)]
mod test {

    use super::{ItemSortKey, watched_pairs};

    /// The live subscription's `Memo` only holds still across a refetch if
    /// the watched set is order-independent and free of the duplicates a
    /// retainer with several listings of one item produces.
    #[test]
    fn watched_pairs_is_a_sorted_set() {
        let a = watched_pairs([(34, 5), (34, 5), (40, 9), (34, 7)]);
        let b = watched_pairs([(40, 9), (34, 7), (34, 5)]);
        assert_eq!(a, vec![(34, 5), (34, 7), (40, 9)]);
        assert_eq!(a, b);
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn test_sort_order() {
        // these item ids are in the correct order- so if we run it through our sort, it should still match up
        use chrono::NaiveDateTime;
        use ultros_api_types::ActiveListing;
        let item_ids = vec![
            30842, 31840, 29417, 17325, 9050, 15532, 36837, 4737, 24250, 19853,
        ];
        let mut item_vec: Vec<_> = item_ids
            .into_iter()
            .map(|item| ActiveListing {
                id: 0,
                world_id: 0,
                item_id: item,
                retainer_id: 0,
                price_per_unit: 1000,
                quantity: 1,
                hq: true,
                timestamp: NaiveDateTime::MIN,
            })
            .collect();
        let original = item_vec.clone();
        item_vec.sort_by_key(|i| ItemSortKey::from(i));
        assert_eq!(original, item_vec);
    }

    /// A row is its own `<For>` key, so two listings of the same item must
    /// still be two rows, and a price change must change exactly that row's
    /// key — that is what keeps a refetch from re-creating the other rows.
    #[cfg(feature = "ssr")]
    #[test]
    fn row_key_follows_its_own_listing() {
        use super::{listing_rows, undercut_rows};
        use crate::api::UndercutData;
        use chrono::NaiveDateTime;
        use std::collections::HashSet;
        use ultros_api_types::ActiveListing;
        let listing = |id: i32, price_per_unit: i32| ActiveListing {
            id,
            world_id: 34,
            item_id: 30842,
            retainer_id: 1,
            price_per_unit,
            quantity: 1,
            hq: true,
            timestamp: NaiveDateTime::MIN,
        };
        let before = listing_rows(&[listing(1, 1000), listing(2, 1000)]);
        assert_eq!(before.iter().collect::<HashSet<_>>().len(), 2);

        let after = listing_rows(&[listing(1, 1000), listing(2, 900)]);
        assert_eq!(after[0], before[0]);
        assert_ne!(after[1], before[1]);

        let undercut = |id, price_per_unit, cheapest| UndercutData {
            current: listing(id, price_per_unit),
            cheapest,
        };
        let before = undercut_rows(&[undercut(1, 1000, 999), undercut(2, 1000, 999)]);
        let after = undercut_rows(&[undercut(1, 1000, 999), undercut(2, 1000, 950)]);
        assert_eq!(after[0], before[0]);
        assert_ne!(after[1], before[1]);
        assert_eq!(after[1].undercut_by_one, Some(949));
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn same_sort_category() {
        use xiv_gen::ItemId;

        let expected_order = vec![
            41509, // red corsage
            41516, // black corsage
            41517,
        ]; // rainbow corsage
        let mut rearranged = vec![41516, 41517, 41509];
        rearranged.sort_by_key(|id| ItemSortKey::from((ItemId(*id), true)));
        assert_eq!(expected_order, rearranged);
    }
}
