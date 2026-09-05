use icondata as i;
use leptos::{prelude::*, task::spawn_local};
use ultros_api_types::alert::{Alert, AlertTrigger, Endpoint, UpdateAlertRequest};
use xiv_gen::ItemId;

use crate::api::{delete_alert, get_alerts, list_endpoints, patch_alert};
use crate::components::alert_drawer::AlertDrawer;
use crate::components::data_table::{Column, ColumnHeader, TrackWidths, body_cells, header_cells};
use crate::components::icon::Icon;
use crate::components::skeleton::{SkeletonCell, SkeletonColumn, TableSkeleton};
use crate::global_state::toasts::use_toast;
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::{Locale, t, t_string, use_i18n};
use leptos_i18n::I18nContext;

/// Shared header-cell classes for the alert rules `<table>` substrate — same
/// content-sized-columns case as the retainer tables in `routes/retainers.rs`
/// (see the substrate note in `components/data_table.rs`): no responsive
/// column set to express, so a real `<table>` beats `DataTableGrid`'s div
/// grid here too.
const TH: &str = "text-left p-1 font-bold whitespace-nowrap";

/// One row of the alert rules table, flattened out of the display strings an
/// [`Alert`] needs per trigger variant plus the resolved endpoint names and
/// the row's own actions.
struct AlertRow {
    alert: Alert,
    item_name: String,
    threshold_str: String,
    world_str: String,
    hq_str: String,
    endpoints_str: String,
}

/// Skeleton columns matching [`alert_rules_columns`]'s seven columns, in the
/// same order, so the loading state has the real table's rhythm.
fn alert_rules_skeleton_columns() -> Vec<SkeletonColumn> {
    vec![
        SkeletonColumn::new("flex-1 min-w-32 p-1", SkeletonCell::IconText),
        SkeletonColumn::new("w-24 p-1", SkeletonCell::Text),
        SkeletonColumn::new("w-20 p-1", SkeletonCell::Text),
        SkeletonColumn::new("w-12 p-1", SkeletonCell::Text),
        SkeletonColumn::new("w-32 p-1", SkeletonCell::Text),
        SkeletonColumn::new("w-20 p-1", SkeletonCell::Badge),
        SkeletonColumn::new("w-20 p-1", SkeletonCell::Blank),
    ]
}

/// The seven columns the alert rules table shows, in DOM order — one list
/// driving the header, the body rows and the empty state's `colspan`, same
/// pattern as #1080's item explorer / currency exchange and the retainer
/// tables above.
fn alert_rules_columns(
    i18n: I18nContext<Locale, crate::i18n::I18nKeys>,
    toggle: impl Fn(Alert) + Copy + Send + Sync + 'static,
    remove: impl Fn(i32) + Copy + Send + Sync + 'static,
) -> Vec<Column<AlertRow>> {
    vec![
        Column::new(
            TrackWidths::default(),
            ColumnHeader::content(move || view! { {t!(i18n, item)} }.into_any()),
            |row: &AlertRow| view! { <td class="p-1">{row.item_name.clone()}</td> }.into_any(),
        )
        .header_class(TH),
        Column::new(
            TrackWidths::default(),
            ColumnHeader::content(move || {
                view! { {t!(i18n, alert_rules_col_threshold)} }.into_any()
            }),
            |row: &AlertRow| view! { <td class="p-1">{row.threshold_str.clone()}</td> }.into_any(),
        )
        .header_class(TH),
        Column::new(
            TrackWidths::default(),
            ColumnHeader::content(move || view! { {t!(i18n, world)} }.into_any()),
            |row: &AlertRow| view! { <td class="p-1">{row.world_str.clone()}</td> }.into_any(),
        )
        .header_class(TH),
        Column::new(
            TrackWidths::default(),
            ColumnHeader::content(move || view! { {t!(i18n, hq)} }.into_any()),
            |row: &AlertRow| view! { <td class="p-1">{row.hq_str.clone()}</td> }.into_any(),
        )
        .header_class(TH),
        Column::new(
            TrackWidths::default(),
            ColumnHeader::content(move || view! { {t!(i18n, endpoints_heading)} }.into_any()),
            |row: &AlertRow| view! { <td class="p-1">{row.endpoints_str.clone()}</td> }.into_any(),
        )
        .header_class(TH),
        Column::new(
            TrackWidths::default(),
            ColumnHeader::content(move || view! { {t!(i18n, status_label)} }.into_any()),
            move |row: &AlertRow| {
                let enabled = row.alert.enabled;
                view! {
                    <td class="p-1">
                        {if enabled {
                            t_string!(i18n, alerts_status_enabled).to_string()
                        } else {
                            t_string!(i18n, alerts_status_disabled).to_string()
                        }}
                    </td>
                }
                .into_any()
            },
        )
        .header_class(TH),
        Column::new(
            TrackWidths::default(),
            ColumnHeader::content(move || view! { {t!(i18n, actions)} }.into_any()),
            move |row: &AlertRow| {
                let enabled = row.alert.enabled;
                let alert = row.alert.clone();
                let id = row.alert.id;
                view! {
                    <td class="p-1 flex gap-1">
                        <button
                            class="btn-ghost"
                            aria-label=t_string!(i18n, alert_rules_aria_toggle_enabled)
                            on:click=move |_| toggle(alert.clone())
                        >
                            <Icon icon=if enabled { i::BsPauseFill } else { i::BsPlayFill } />
                        </button>
                        <button
                            class="btn-ghost text-red-400"
                            aria-label=t_string!(i18n, alert_rules_aria_delete_alert)
                            on:click=move |_| remove(id)
                        >
                            <Icon icon=i::BiTrashSolid />
                        </button>
                    </td>
                }
                .into_any()
            },
        )
        .header_class(TH),
    ]
}

#[component]
pub fn AlertRulesPanel() -> impl IntoView {
    let i18n = use_i18n();
    let version = RwSignal::new(0u64);
    let alerts = Resource::new(move || version.get(), move |_| get_alerts());
    let endpoints = Resource::new(move || version.get(), move |_| list_endpoints());
    let toasts = use_toast();
    let (drawer_visible, set_drawer_visible) = signal(false);

    // Refresh the alerts list when the drawer closes (best-effort: we can't tell if
    // the user actually saved without threading a callback, so we just bump the
    // version on every close — cheap enough).
    Effect::new(move |_| {
        let visible = drawer_visible.get();
        if !visible {
            version.update(|v| *v += 1);
        }
    });

    let toggle = move |alert: Alert| {
        let new_enabled = !alert.enabled;
        spawn_local(async move {
            match patch_alert(
                alert.id,
                UpdateAlertRequest {
                    enabled: Some(new_enabled),
                    price_threshold: None,
                    endpoint_ids: None,
                    cooldown_seconds: None,
                },
            )
            .await
            {
                Ok(()) => {
                    if let Some(t) = toasts {
                        t.success(if new_enabled {
                            t_string!(i18n, alerts_alert_enabled).to_string()
                        } else {
                            t_string!(i18n, alerts_alert_disabled).to_string()
                        });
                    }
                    version.update(|v| *v += 1);
                }
                Err(e) => {
                    if let Some(t) = toasts {
                        t.error(format!("{e}"));
                    }
                }
            }
        });
    };

    let remove = move |id: i32| {
        spawn_local(async move {
            match delete_alert(id).await {
                Ok(()) => {
                    if let Some(t) = toasts {
                        t.success(t_string!(i18n, alerts_alert_deleted).to_string());
                    }
                    version.update(|v| *v += 1);
                }
                Err(e) => {
                    if let Some(t) = toasts {
                        t.error(format!("{e}"));
                    }
                }
            }
        });
    };

    view! {
        <div class="space-y-3">
            <div class="flex justify-end gap-2">
                <button class="btn" on:click=move |_| set_drawer_visible.set(true)>
                    <Icon icon=i::BsBell />
                    <span class="ml-1">{t!(i18n, add_alert_button)}</span>
                </button>
            </div>
            <Show when=move || drawer_visible.get()>
                <AlertDrawer set_visible=set_drawer_visible.into() />
            </Show>
            <Suspense fallback=move || {
                view! { <TableSkeleton columns=alert_rules_skeleton_columns() rows=4 /> }
            }>
            {move || {
                let endpoint_list: Vec<Endpoint> = endpoints
                    .get()
                    .and_then(|r| r.ok())
                    .unwrap_or_default();
                let ep_name = move |id: i32| {
                    endpoint_list
                        .iter()
                        .find(|e| e.id == id)
                        .map(|e| e.name.clone())
                        .unwrap_or_else(|| format!("#{id}"))
                };
                alerts
                    .get()
                    .map(|r| match r {
                        Ok(rows) if rows.is_empty() => {
                            view! {
                                <p class="opacity-70">
                                    {t!(i18n, alerts_empty_state)}
                                </p>
                            }
                                .into_any()
                        }
                        Ok(rows) => {
                            let columns = alert_rules_columns(i18n, toggle, remove);
                            let rows: Vec<AlertRow> = rows
                                .into_iter()
                                .map(|a: Alert| {
                                    // Display strings differ per trigger variant. List-scoped alerts
                                    // don't carry a single item/world/hq — render those columns with
                                    // the list id and "—" placeholders so the table stays uniform.
                                    let (item_name, threshold_str, world_str, hq_str): (
                                        String,
                                        String,
                                        String,
                                        String,
                                    ) = match a.trigger.clone() {
                                        AlertTrigger::BelowThreshold {
                                            item_id,
                                            price_threshold,
                                            hq_only,
                                            world_selector,
                                        } => {
                                            let name = tracked_data()
                                                .items
                                                .get(&ItemId(item_id))
                                                .map(|it| it.name.as_str().to_string())
                                                .unwrap_or_else(|| format!("Item {item_id}"));
                                            let threshold = format!("≤ {price_threshold} gil");
                                            let world = match world_selector {
                                                ultros_api_types::world_helper::AnySelector::World(id) => {
                                                    format!("World({id})")
                                                }
                                                ultros_api_types::world_helper::AnySelector::Datacenter(id) => {
                                                    format!("DC({id})")
                                                }
                                                ultros_api_types::world_helper::AnySelector::Region(id) => {
                                                    format!("Region({id})")
                                                }
                                            };
                                            let hq = if hq_only {
                                                t_string!(i18n, alerts_hq_any).to_string()
                                            } else {
                                                t_string!(i18n, alerts_any).to_string()
                                            };
                                            (name, threshold, world, hq)
                                        }
                                        AlertTrigger::ListItemThreshold { list_id } => (
                                            format!("List #{list_id}"),
                                            t_string!(i18n, alerts_list_price_target).to_string(),
                                            t_string!(i18n, alerts_list_defined_world).to_string(),
                                            "—".to_string(),
                                        ),
                                        AlertTrigger::RetainerUndercut { margin_percent } => (
                                            t_string!(i18n, alerts_retainer_undercut_rule).to_string(),
                                            t_string!(i18n, alerts_margin_percent, margin = margin_percent).to_string(),
                                            "—".to_string(),
                                            "—".to_string(),
                                        ),
                                        AlertTrigger::ListUpdate { list_id } => (
                                            format!("List #{list_id}"),
                                            t_string!(i18n, alerts_list_update_rule).to_string(),
                                            "—".to_string(),
                                            "—".to_string(),
                                        ),
                                    };
                                    let endpoints_str = a
                                        .endpoint_ids
                                        .iter()
                                        .map(|id| ep_name(*id))
                                        .collect::<Vec<_>>()
                                        .join(", ");
                                    AlertRow {
                                        alert: a,
                                        item_name,
                                        threshold_str,
                                        world_str,
                                        hq_str,
                                        endpoints_str,
                                    }
                                })
                                .collect();
                            view! {
                                <div class="overflow-x-auto">
                                    <table class="w-full text-sm text-left">
                                        <thead>
                                            <tr>{header_cells(&columns)}</tr>
                                        </thead>
                                        <tbody>
                                            {rows
                                                .into_iter()
                                                .map(|row| {
                                                    view! {
                                                        <tr class="border-t">{body_cells(&columns, &row)}</tr>
                                                    }
                                                })
                                                .collect::<Vec<_>>()}
                                        </tbody>
                                    </table>
                                </div>
                            }
                                .into_any()
                        }
                        Err(e) => {
                            view! { <div class="text-red-500">{format!("{e}")}</div> }.into_any()
                        }
                    })
            }}
            </Suspense>
        </div>
    }
}
