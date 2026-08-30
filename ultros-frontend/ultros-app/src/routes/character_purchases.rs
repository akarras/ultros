//! `/retainers/purchases` — the buy side of the market board, for characters
//! the signed-in user has claimed.
//!
//! Ultros records the buyer on every sale it ingests, and the item page has
//! always rendered that name. This page reads the same rows by buyer, so you
//! can go back and see what you actually paid for something.
//!
//! Two limits are stated on the page rather than hidden, because both are
//! properties of the upstream data and neither can be engineered away:
//!
//! - **Identity.** Universalis reports a buyer as a bare character name with no
//!   world attached, so same-named characters elsewhere in the region share one
//!   buyer record. Claiming proves who you are; it does not make the rows
//!   yours.
//! - **Coverage.** A sale is only here if someone uploaded that board while the
//!   sale was still in its recent history. Gaps are invisible — a missing
//!   purchase looks exactly like a purchase that never happened.

use crate::api::{get_character_purchases, get_characters, get_login};
use crate::components::app_link::AppLink;
use crate::components::data_table::{
    Column, ColumnHeader, TrackWidths, body_cells, header_cells, visible_column_count,
};
use crate::components::gil::*;
use crate::components::item_icon::*;
use crate::components::meta::*;
use crate::components::relative_time::*;
use crate::components::skeleton::{SkeletonCell, SkeletonColumn, TableSkeleton};
use crate::components::tool_help::ActionableEmptyState;
use crate::components::world_name::*;
use crate::global_state::use_world_display_name;
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;
use leptos::either::Either;
use leptos::prelude::*;
use leptos_i18n::I18nContext;
use leptos_router::hooks::query_signal;
use std::sync::Arc;
use thousands::Separable;
use ultros_api_types::character_purchases::{CharacterPurchase, CharacterPurchaseHistory};
use ultros_api_types::icon_size::IconSize;
use ultros_api_types::{FfxivCharacter, world_helper::AnySelector};
use xiv_gen::ItemId;

/// Shared header-cell classes, matching the retainer tables this page sits
/// beside in the same tab shell.
const TH: &str = "px-3 py-2 font-bold whitespace-nowrap text-left";

/// Skeleton mirroring the real table's seven columns, so the loading state has
/// the same rhythm as the content that replaces it.
fn purchase_skeleton_columns() -> Vec<SkeletonColumn> {
    vec![
        SkeletonColumn::new("w-10 px-3 py-2", SkeletonCell::Blank),
        SkeletonColumn::new("flex-1 min-w-40 px-3 py-2", SkeletonCell::IconText),
        SkeletonColumn::new("w-24 px-3 py-2", SkeletonCell::Number),
        SkeletonColumn::new("w-16 px-3 py-2", SkeletonCell::Number),
        SkeletonColumn::new("w-24 px-3 py-2", SkeletonCell::Number),
        SkeletonColumn::new("w-24 px-3 py-2", SkeletonCell::Text),
        SkeletonColumn::new("w-28 px-3 py-2", SkeletonCell::Text),
    ]
}

fn purchase_columns(
    i18n: I18nContext<Locale, I18nKeys>,
    world_name: Arc<str>,
) -> Vec<Column<CharacterPurchase>> {
    vec![
        Column::new(
            TrackWidths::default(),
            ColumnHeader::content(move || view! { {t!(i18n, retainers_hq)} }.into_any()),
            move |row: &CharacterPurchase| {
                view! { <td class="px-3 py-2">{row.hq.then_some(t!(i18n, retainers_hq))}</td> }
                    .into_any()
            },
        )
        .header_class(TH),
        Column::new(
            TrackWidths::default(),
            ColumnHeader::content(move || view! { {t!(i18n, retainers_item)} }.into_any()),
            move |row: &CharacterPurchase| {
                let item = tracked_data().items.get(&ItemId(row.item_id));
                let item_id = row.item_id;
                let world_name = world_name.clone();
                view! {
                    <td class="px-3 py-2">
                        {if let Some(item) = item {
                            Either::Left(
                                view! {
                                    <AppLink
                                        attr:class="flex flex-row items-center gap-1"
                                        href=format!("/item/{world_name}/{item_id}")
                                    >
                                        <ItemIcon icon_size=IconSize::Small item_id=item_id />
                                        {item.name.as_str()}
                                    </AppLink>
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
            move |row: &CharacterPurchase| {
                view! {
                    <td class="px-3 py-2">
                        <Gil amount=row.price_per_item />
                    </td>
                }
                .into_any()
            },
        )
        .header_class(TH),
        Column::new(
            TrackWidths::default(),
            ColumnHeader::content(move || view! { {t!(i18n, retainers_quantity)} }.into_any()),
            move |row: &CharacterPurchase| {
                view! { <td class="px-3 py-2">{row.quantity}</td> }.into_any()
            },
        )
        .header_class(TH),
        Column::new(
            TrackWidths::default(),
            ColumnHeader::content(move || view! { {t!(i18n, retainers_total)} }.into_any()),
            move |row: &CharacterPurchase| {
                // Saturating rather than wrapping: a full stack of a
                // max-priced item overflows `i32`, and `<Gil>` takes `i32`.
                let total = i32::try_from(row.total_gil()).unwrap_or(i32::MAX);
                view! {
                    <td class="px-3 py-2">
                        <Gil amount=total />
                    </td>
                }
                .into_any()
            },
        )
        .header_class(TH),
        Column::new(
            TrackWidths::default(),
            ColumnHeader::content(move || {
                view! { {t!(i18n, character_purchases_col_bought_on)} }.into_any()
            }),
            move |row: &CharacterPurchase| {
                view! {
                    <td class="px-3 py-2">
                        <WorldName id=AnySelector::World(row.world_id) />
                    </td>
                }
                .into_any()
            },
        )
        .header_class(TH),
        Column::new(
            TrackWidths::default(),
            ColumnHeader::content(move || {
                view! { {t!(i18n, character_purchases_col_when)} }.into_any()
            }),
            move |row: &CharacterPurchase| {
                view! {
                    <td class="px-3 py-2">
                        <RelativeToNow timestamp=row.sold_date />
                    </td>
                }
                .into_any()
            },
        )
        .header_class(TH),
    ]
}

/// One headline number in the summary strip.
#[component]
fn SummaryTile(label: String, value: String) -> impl IntoView {
    view! {
        <div class="panel p-4 rounded-xl flex flex-col gap-1 min-w-40">
            <span class="text-xs uppercase tracking-wider text-[color:var(--color-text-muted)]">
                {label}
            </span>
            <span class="text-xl font-bold text-[color:var(--brand-fg)]">{value}</span>
        </div>
    }
}

#[component]
fn PurchaseTable(history: CharacterPurchaseHistory) -> impl IntoView {
    let i18n = use_i18n();
    // The item links need *a* world in their path; the character's own world
    // is the one the user thinks in, even though a given row may have been
    // bought elsewhere in the region (the row's own world is its own column).
    let world_name: Arc<str> =
        use_world_display_name(AnySelector::World(history.character.world_id))
            .unwrap_or_default()
            .into();
    let columns = purchase_columns(i18n, world_name);
    let column_count = visible_column_count(&columns).to_string();
    let summary = history.summary;
    let is_empty = history.purchases.is_empty();
    let truncated = history.truncated;
    let shown = history.purchases.len();
    let worlds_searched = history.scoped_world_ids.len();

    view! {
        <div class="flex flex-col gap-4">
            <div class="flex flex-row flex-wrap gap-3">
                <SummaryTile
                    label=t_string!(i18n, character_purchases_stat_purchases).to_string()
                    value=summary.total_purchases.separate_with_commas()
                />
                <SummaryTile
                    label=t_string!(i18n, character_purchases_stat_gil).to_string()
                    value=summary.total_gil.separate_with_commas()
                />
                <SummaryTile
                    label=t_string!(i18n, character_purchases_stat_items).to_string()
                    value=summary.distinct_items.separate_with_commas()
                />
                <SummaryTile
                    label=t_string!(i18n, character_purchases_stat_units).to_string()
                    value=summary.total_units.separate_with_commas()
                />
            </div>

            // Both caveats live above the table rather than in a footnote:
            // a reader who takes these numbers at face value is being
            // misled, and the table is the thing they'd take at face value.
            <div class="panel p-4 rounded-xl text-sm flex flex-col gap-2">
                <p>{t!(i18n, character_purchases_identity_notice)}</p>
                <p class="text-[color:var(--color-text-muted)]">
                    {t!(i18n, character_purchases_coverage_notice)}
                </p>
                <p class="text-[color:var(--color-text-muted)]">
                    {t!(i18n, character_purchases_scope_notice, count = worlds_searched)}
                </p>
            </div>

            {truncated
                .then(|| {
                    view! {
                        <div class="text-sm text-[color:var(--color-text-muted)]">
                            {t!(
                                i18n, character_purchases_truncated, shown = shown, total = summary
                                .total_purchases as usize
                            )}
                        </div>
                    }
                })}

            <div class="panel p-4 rounded-xl">
                <div class="overflow-x-auto">
                    <table class="w-full text-sm text-left">
                        <thead class="text-xs font-bold uppercase tracking-wider text-[color:var(--color-text-muted)]">
                            <tr class="border-b border-white/5">{header_cells(&columns)}</tr>
                        </thead>
                        <tbody class="divide-y divide-white/5">
                            {is_empty
                                .then(|| {
                                    view! {
                                        <tr>
                                            <td
                                                colspan=column_count.clone()
                                                class="p-4 text-center opacity-70"
                                            >
                                                {t!(i18n, character_purchases_empty)}
                                            </td>
                                        </tr>
                                    }
                                })}
                            {history
                                .purchases
                                .into_iter()
                                .map(|row| {
                                    view! {
                                        <tr class="hover:bg-white/5 transition-colors">
                                            {body_cells(&columns, &row)}
                                        </tr>
                                    }
                                })
                                .collect::<Vec<_>>()}
                        </tbody>
                    </table>
                </div>
            </div>
        </div>
    }
}

/// Character picker. Rendered even for a single character so the page always
/// says whose purchases are on screen.
#[component]
fn CharacterPicker(
    characters: Vec<FfxivCharacter>,
    active: Memo<Option<i32>>,
    set_character: SignalSetter<Option<i32>>,
) -> impl IntoView {
    view! {
        <div class="flex flex-row flex-wrap gap-2">
            {characters
                .into_iter()
                .map(|character| {
                    let id = character.id;
                    let label = format!("{} {}", character.first_name, character.last_name);
                    let class = move || {
                        if active() == Some(id) {
                            "btn-primary"
                        } else {
                            "btn-secondary"
                        }
                    };
                    view! {
                        <button class=class on:click=move |_| set_character.set(Some(id))>
                            {label}
                        </button>
                    }
                })
                .collect::<Vec<_>>()}
        </div>
    }
}

#[component]
pub fn CharacterPurchases() -> impl IntoView {
    let i18n = use_i18n();
    let login = Resource::new(|| (), |_| async move { get_login().await });
    let characters = Resource::new(
        move || login.get().map(|res| res.is_ok()).unwrap_or(false),
        move |logged_in| async move {
            if logged_in {
                get_characters().await
            } else {
                Err(crate::error::AppError::ApiError(
                    ultros_api_types::result::ApiError::NotAuthenticated,
                ))
            }
        },
    );
    // Kept in the URL so a given character's history is linkable and survives
    // a refresh.
    let (character_param, set_character) = query_signal::<i32>("character");
    // Falling back to the first claimed character means the page shows
    // something useful on arrival rather than an empty picker.
    let active_character = Memo::new(move |_| {
        character_param.get().or_else(|| {
            characters
                .get()
                .and_then(|res| res.ok())
                .and_then(|list| list.first().map(|c| c.id))
        })
    });
    let purchases = Resource::new(
        move || active_character.get(),
        move |character_id| async move {
            match character_id {
                Some(id) => get_character_purchases(id).await.map(Some),
                None => Ok(None),
            }
        },
    );

    view! {
        <span class="content-title">{t!(i18n, character_purchases_title)}</span>
        <MetaTitle title=t_string!(i18n, character_purchases_title).to_string() />
        <MetaDescription text=t_string!(i18n, character_purchases_desc).to_string() />
        <br />
        <Suspense fallback=move || {
            view! { <TableSkeleton columns=purchase_skeleton_columns() rows=5 /> }
        }>
            {move || {
                match login.get() {
                    None => {
                        view! { <TableSkeleton columns=purchase_skeleton_columns() rows=5 /> }
                            .into_any()
                    }
                    Some(Err(_)) => {
                        view! {
                            <ActionableEmptyState
                                title=t_string!(i18n, character_purchases_signed_out_title)
                                    .to_string()
                                body=t_string!(i18n, character_purchases_signed_out_body)
                                    .to_string()
                                action_href="/login?next=/retainers/purchases"
                                action_label=t_string!(i18n, sign_in_discord).to_string()
                                action_external=true
                            />
                        }
                            .into_any()
                    }
                    Some(Ok(_)) => {
                        view! {
                            {move || {
                                match characters.get() {
                                    None => {
                                        view! {
                                            <TableSkeleton columns=purchase_skeleton_columns() rows=5 />
                                        }
                                            .into_any()
                                    }
                                    Some(Err(e)) => {
                                        view! {
                                            <div class="alert alert-error">
                                                {t!(i18n, character_purchases_load_error, error = e.to_string())}
                                            </div>
                                        }
                                            .into_any()
                                    }
                                    Some(Ok(list)) if list.is_empty() => {
                                        view! {
                                            <ActionableEmptyState
                                                title=t_string!(i18n, character_purchases_no_characters_title)
                                                    .to_string()
                                                body=t_string!(i18n, character_purchases_no_characters_body)
                                                    .to_string()
                                                action_href="/retainers/edit"
                                                action_label=t_string!(i18n, character_purchases_claim_action)
                                                    .to_string()
                                            />
                                        }
                                            .into_any()
                                    }
                                    Some(Ok(list)) => {
                                        view! {
                                            <div class="flex flex-col gap-4">
                                                <CharacterPicker
                                                    characters=list
                                                    active=active_character
                                                    set_character=set_character
                                                />
                                                <Suspense fallback=move || {
                                                    view! {
                                                        <TableSkeleton
                                                            columns=purchase_skeleton_columns()
                                                            rows=5
                                                        />
                                                    }
                                                }>
                                                    {move || {
                                                        match purchases.get() {
                                                            None | Some(Ok(None)) => {
                                                                view! {
                                                                    <TableSkeleton
                                                                        columns=purchase_skeleton_columns()
                                                                        rows=5
                                                                    />
                                                                }
                                                                    .into_any()
                                                            }
                                                            Some(Ok(Some(history))) => {
                                                                view! { <PurchaseTable history /> }.into_any()
                                                            }
                                                            Some(Err(e)) => {
                                                                view! {
                                                                    <div class="alert alert-error">
                                                                        {t!(
                                                                            i18n, character_purchases_load_error, error = e.to_string()
                                                                        )}
                                                                    </div>
                                                                }
                                                                    .into_any()
                                                            }
                                                        }
                                                    }}
                                                </Suspense>
                                            </div>
                                        }
                                            .into_any()
                                    }
                                }
                            }}
                        }
                            .into_any()
                    }
                }
            }}
        </Suspense>
    }
    .into_any()
}
