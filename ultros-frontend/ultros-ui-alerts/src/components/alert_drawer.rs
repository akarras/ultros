//! Unified "Add alert" modal shared by the /alerts rules panel, the
//! /retainers/undercuts page, and per-item entry points (item page, list
//! rows). A type toggle switches between the alert shapes (item price
//! threshold, below median, back in stock, retainer undercut, retainer sale),
//! and the same surface lists the user's existing alerts of the selected type
//! with a remove button, so pages embedding this drawer don't need their own
//! manager UI.
//!
//! When opened with a `preset_item` (a specific item id/name already known,
//! e.g. from a list row) the item search UI is hidden and the type toggle
//! only offers the item-scoped kinds, all for that one item. This merges the
//! old, now-deleted `AlertConfigDrawer` into this one (#1130): same
//! `<Modal>`, same fields, same `alert_drawer_*` i18n keys, previously
//! duplicated across two files.

use icondata as i;
use leptos::{prelude::*, reactive::wrappers::write::SignalSetter, task::spawn_local};
use leptos_i18n::I18nContext;
use std::cmp::Reverse;
use std::collections::HashSet;
use thousands::Separable;
use ultros_api_types::{
    alert::{
        Alert, AlertTrigger, BACK_IN_STOCK_MIN_EMPTY_SECS, BELOW_MEDIAN_PERCENT_RANGE,
        CreateAlertRequest, Endpoint, EndpointMethod, QualityBaselines, baseline_is_usable,
    },
    icon_size::IconSize,
    item_stats::ItemStatsVariant,
    world_helper::AnySelector,
};
use xiv_gen::{Item, ItemId};

use crate::api::{create_alert, delete_alert, get_alerts, get_item_stats, list_endpoints};
use crate::components::{
    endpoint_picker::EndpointPicker, icon::Icon, item_icon::ItemIcon, modal::Modal,
    world_picker::WorldPicker,
};
use crate::global_state::guest_alerts::{GuestAlertRule, use_guest_alerts};
use crate::global_state::home_world::use_home_world;
use crate::global_state::local_world_data::{
    LocalWorldData, use_world_display_name, world_display_name_from_context,
};
use crate::global_state::toasts::use_toast;
use crate::global_state::user::BootstrapUser;
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::{Locale, t, t_string, use_i18n};

/// Which alert shape the drawer is currently configuring.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AlertKind {
    #[default]
    ItemPrice,
    BelowMedian,
    BackInStock,
    Undercut,
    Sold,
}

impl AlertKind {
    /// Kinds configured against one item in one world/DC/region scope: they
    /// share the item picker, world picker and HQ toggle.
    fn is_item_scoped(self) -> bool {
        matches!(
            self,
            AlertKind::ItemPrice | AlertKind::BelowMedian | AlertKind::BackInStock
        )
    }
}

/// Default for the below-median percent input.
const DEFAULT_PERCENT_BELOW: i32 = 30;

/// Whether an existing alert belongs under the given drawer tab. List-scoped
/// alerts are managed from their list pages and never show here.
fn trigger_matches_kind(trigger: &AlertTrigger, kind: AlertKind) -> bool {
    matches!(
        (trigger, kind),
        (AlertTrigger::BelowThreshold { .. }, AlertKind::ItemPrice)
            | (AlertTrigger::BelowMedian { .. }, AlertKind::BelowMedian)
            | (AlertTrigger::BackInStock { .. }, AlertKind::BackInStock)
            | (AlertTrigger::RetainerUndercut { .. }, AlertKind::Undercut)
            | (AlertTrigger::RetainerSold {}, AlertKind::Sold)
    )
}

/// Alert kinds offered by the drawer's type toggle. A signed-out guest can
/// only manage client-side item-price alerts (every other kind needs server
/// state — retainer ownership, a sales baseline, how long a board has been
/// empty), so guest mode locks the toggle to just that one kind. A drawer
/// opened for one preset item offers only the item-scoped kinds.
fn kinds_for(signed_in: bool, item_locked: bool) -> &'static [AlertKind] {
    match (signed_in, item_locked) {
        (false, _) => &[AlertKind::ItemPrice],
        (true, true) => &[
            AlertKind::ItemPrice,
            AlertKind::BelowMedian,
            AlertKind::BackInStock,
        ],
        (true, false) => &[
            AlertKind::ItemPrice,
            AlertKind::BelowMedian,
            AlertKind::BackInStock,
            AlertKind::Undercut,
            AlertKind::Sold,
        ],
    }
}

/// The below-median percent input, if it's a whole number the server will
/// accept ([`BELOW_MEDIAN_PERCENT_RANGE`]).
fn parse_percent_below(input: &str) -> Option<i32> {
    input
        .trim()
        .parse::<i32>()
        .ok()
        .filter(|p| BELOW_MEDIAN_PERCENT_RANGE.contains(p))
}

/// The baselines a below-median alert would actually compare listings with,
/// NQ first: just HQ for an HQ-only alert, otherwise both qualities (each
/// listing is measured against its own quality's median). Unusable baselines
/// are dropped — the server never fires against them.
fn usable_baselines(baselines: &QualityBaselines, hq_only: bool) -> Vec<&ItemStatsVariant> {
    let qualities: &[bool] = if hq_only { &[true] } else { &[false, true] };
    qualities
        .iter()
        .filter_map(|hq| baselines.for_quality(*hq))
        .filter(|b| baseline_is_usable(b))
        .collect()
}

/// One row of the "Active" list — shared markup for both a signed-in
/// server-backed [`Alert`] and a guest's local [`GuestAlertRule`], which
/// differ only in what goes in `description`/`sub_label` and what `on_delete`
/// does (`delete_alert` over the network vs. `GuestAlerts::remove` in
/// `localStorage`).
fn active_alert_row(
    description: String,
    sub_label: String,
    delete_aria: String,
    on_delete: impl Fn() + 'static,
) -> impl IntoView {
    view! {
        <li class="flex items-center justify-between gap-2 p-2">
            <div class="min-w-0">
                <div class="text-sm truncate">{description}</div>
                <div class="text-xs opacity-60 truncate">{sub_label}</div>
            </div>
            <button
                class="btn-ghost text-red-400"
                aria-label=delete_aria
                on:click=move |_| on_delete()
            >
                <Icon icon=i::BiTrashSolid />
            </button>
        </li>
    }
}

/// "Active" list description for an item-scoped alert row: item name, the
/// kind's condition (price threshold, percent below median, back in stock),
/// world/datacenter/region and the HQ-only marker. Shared by the guest list
/// (built from a [`GuestAlertRule`]) and the signed-in list (built from an
/// item-scoped `AlertTrigger`), which carry the same fields — factoring this
/// out both gets the world/HQ text onto the guest rows (without it, two rules
/// on the same item that differ only by world or HQ render identical text)
/// and removes what was a duplicated item-name lookup between the branches.
fn item_row_description(
    item_id: i32,
    condition: &str,
    world_selector: AnySelector,
    hq_only: bool,
    i18n: I18nContext<Locale, crate::i18n::I18nKeys>,
) -> String {
    let name = tracked_data()
        .items
        .get(&ItemId(item_id))
        .map(|it| it.name.as_str().to_string())
        .unwrap_or_else(|| format!("Item {item_id}"));
    let world = use_world_display_name(world_selector).unwrap_or_else(|| {
        let (AnySelector::World(id) | AnySelector::Datacenter(id) | AnySelector::Region(id)) =
            world_selector;
        format!("#{id}")
    });
    let hq = if hq_only {
        t_string!(i18n, alerts_hq_any).to_string()
    } else {
        t_string!(i18n, alerts_any).to_string()
    };
    format!("{name} · {condition} · {world} · {hq}")
}

#[component]
pub fn AlertDrawer(
    #[prop(optional)] initial_kind: AlertKind,
    /// An item already chosen by the caller (id, display name) — e.g. a list
    /// row's own item. When set, the item search UI is hidden and the kind
    /// toggle offers only the item-scoped kinds (item price, below median,
    /// back in stock) for this item — never undercut/sale, which aren't
    /// about one item.
    #[prop(optional)]
    preset_item: Option<(i32, String)>,
    /// Default world selector for the form. When omitted, defaults to the
    /// user's home world (if set). Pass `Signal::derive(|| None)` explicitly
    /// to opt out of the home-world default — used by callers that don't
    /// want to presume a world for an item-scoped alert.
    #[prop(optional, into)]
    default_world: Option<Signal<Option<AnySelector>>>,
    set_visible: SignalSetter<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    // Decided synchronously from context, identically on SSR and the client
    // hydration pass — no Suspense divergence, no flash of the wrong mode.
    // `unwrap_or(true)` keeps the pre-guest-mode behaviour (full drawer) for
    // any caller/test that doesn't provide a `BootstrapUser` context at all.
    let signed_in = use_context::<BootstrapUser>()
        .map(|u| u.0.is_some())
        .unwrap_or(true);
    let guest = use_guest_alerts();
    let locked_to_preset_item = preset_item.is_some();
    let available_kinds = kinds_for(signed_in, locked_to_preset_item);
    let show_kind_picker = available_kinds.len() > 1;
    let kind = RwSignal::new(if available_kinds.contains(&initial_kind) {
        initial_kind
    } else {
        AlertKind::ItemPrice
    });
    // Cache-buster bumped after a delete so the active list refreshes without
    // closing the drawer.
    let version = RwSignal::new(0u64);
    // A guest never hits an authenticated endpoint: these resources simply
    // don't exist in guest mode rather than being created and left unused.
    let alerts = signed_in.then(|| Resource::new(move || version.get(), move |_| get_alerts()));
    let endpoints = signed_in.then(|| Resource::new(|| (), |_| list_endpoints()));
    let selected_endpoints = RwSignal::new(HashSet::<i32>::new());
    let (error, set_error) = signal::<Option<String>>(None);
    let toasts = use_toast();

    // Seed the endpoint selection with the caller's auto-created `InApp`
    // endpoint the first time the endpoints resource resolves, so a
    // signed-in user doesn't have to manually tick a box for the common
    // case. Guarded by `seeded` so it only ever seeds once — if the user
    // deselects everything afterward, it stays deselected.
    if let Some(endpoints) = endpoints {
        let seeded = RwSignal::new(false);
        Effect::new(move |_| {
            if seeded.get_untracked() {
                return;
            }
            let Some(Ok(list)) = endpoints.get() else {
                return;
            };
            if !selected_endpoints.get_untracked().is_empty() {
                return;
            }
            if let Some(in_app) = list
                .iter()
                .find(|e| matches!(e.method, EndpointMethod::InApp {}))
            {
                selected_endpoints.update(|s| {
                    s.insert(in_app.id);
                });
                seeded.set(true);
            }
        });
    }

    // Item-scoped form state (item price, below median, back in stock).
    // Default the world picker to the caller-supplied default, falling back
    // to the user's home world when set.
    let (home_world, _) = use_home_world();
    let (search, set_search) = signal::<String>("".into());
    let selected_item = RwSignal::<Option<(i32, String)>>::new(preset_item.clone());
    let initial_world = default_world
        .map(|s| s.get_untracked())
        .unwrap_or_else(|| home_world.get_untracked().map(|w| AnySelector::World(w.id)));
    let (world, set_world) = signal::<Option<AnySelector>>(initial_world);
    let (price_threshold, set_price_threshold) = signal::<String>("".into());
    let (hq_only, set_hq_only) = signal(false);

    // Below-median form state. Validated as the user types so the submit
    // button can be disabled while the value is outside what the server
    // accepts.
    let (percent_below, set_percent_below) = signal(DEFAULT_PERCENT_BELOW.to_string());
    let percent_below_valid = move || parse_percent_below(&percent_below.get()).is_some();
    let percent_range_error = move || {
        t_string!(
            i18n,
            below_median_alert_err_percent_range,
            min = *BELOW_MEDIAN_PERCENT_RANGE.start(),
            max = *BELOW_MEDIAN_PERCENT_RANGE.end()
        )
        .to_string()
    };

    // 30-day baseline preview for a below-median alert on the chosen item and
    // scope. Client-only (the drawer opens on a click, never in SSR output)
    // and soft-failing like the item page's ConfidenceBadge: a fetch error
    // just shows no preview. `hq_only` isn't part of the key — both qualities
    // come back in one response and the view picks between them.
    let world_data = use_context::<LocalWorldData>();
    let baselines = LocalResource::new(move || {
        let wanted = kind.get() == AlertKind::BelowMedian;
        let item_id = selected_item.get().map(|(id, _)| id);
        let scope = world
            .get()
            .and_then(|w| world_display_name_from_context(world_data.clone(), w));
        async move {
            let (true, Some(item_id), Some(scope)) = (wanted, item_id, scope) else {
                return None;
            };
            let stats = get_item_stats(&scope, item_id).await.ok()?;
            Some((scope, QualityBaselines::from_variants(stats.variants)))
        }
    });

    // Undercut form state.
    let (margin, set_margin) = signal("0".to_string());

    // Same scoring shape as routes/list_view.rs — substring match, descending by
    // item level so common high-IL items rank ahead of vendor trash that shares
    // a prefix. We cap at 50 matches so the dropdown stays usable.
    let item_results = move || -> Vec<(ItemId, &'static Item)> {
        let s = search.get();
        if s.trim().is_empty() {
            return Vec::new();
        }
        let s_lower = s.to_lowercase();
        let items = &tracked_data().items;
        let mut matches: Vec<(&ItemId, &'static Item)> = items
            .iter()
            .filter(|(_, i)| i.item_search_category > 0)
            .filter(|(_, i)| i.name.to_lowercase().contains(&s_lower))
            .collect();

        // ⚡ Bolt Optimization: Use select_nth_unstable_by_key to avoid O(N log N) full sort
        // when we only need the top 50 results. This reduces time complexity to O(N).
        if matches.len() > 50 {
            matches.select_nth_unstable_by_key(50, |(_, i)| Reverse(i.level_item));
            matches.truncate(50);
        }
        matches.sort_unstable_by_key(|(_, i)| Reverse(i.level_item));

        matches.into_iter().map(|(id, item)| (*id, item)).collect()
    };

    // The item and scope every item-scoped kind needs, or `None` after
    // surfacing which one is missing.
    let item_scope = move || -> Option<(i32, AnySelector)> {
        let Some((item_id, _)) = selected_item.get() else {
            set_error.set(Some(
                t_string!(i18n, create_alert_err_pick_item).to_string(),
            ));
            return None;
        };
        let Some(world_selector) = world.get() else {
            set_error.set(Some(
                t_string!(i18n, alert_drawer_err_pick_world).to_string(),
            ));
            return None;
        };
        Some((item_id, world_selector))
    };

    let submit = move |_| {
        set_error.set(None);
        let trigger = match kind.get() {
            AlertKind::ItemPrice => {
                let Some((item_id, world_selector)) = item_scope() else {
                    return;
                };
                let Ok(threshold) = price_threshold.get().parse::<i32>() else {
                    set_error.set(Some(
                        t_string!(i18n, alert_drawer_err_threshold_int).to_string(),
                    ));
                    return;
                };
                if threshold <= 0 {
                    set_error.set(Some(
                        t_string!(i18n, alert_drawer_err_threshold_positive).to_string(),
                    ));
                    return;
                }
                AlertTrigger::BelowThreshold {
                    item_id,
                    world_selector,
                    price_threshold: threshold,
                    hq_only: hq_only.get(),
                }
            }
            AlertKind::BelowMedian => {
                let Some((item_id, world_selector)) = item_scope() else {
                    return;
                };
                let Some(percent_below) = parse_percent_below(&percent_below.get()) else {
                    set_error.set(Some(percent_range_error()));
                    return;
                };
                AlertTrigger::BelowMedian {
                    item_id,
                    world_selector,
                    percent_below,
                    hq_only: hq_only.get(),
                }
            }
            AlertKind::BackInStock => {
                let Some((item_id, world_selector)) = item_scope() else {
                    return;
                };
                AlertTrigger::BackInStock {
                    item_id,
                    world_selector,
                    hq_only: hq_only.get(),
                }
            }
            AlertKind::Undercut => {
                let Ok(margin_percent) = margin.get().trim().parse::<i32>() else {
                    set_error.set(Some(
                        t_string!(i18n, undercut_alert_err_margin_number).to_string(),
                    ));
                    return;
                };
                if !(0..=200).contains(&margin_percent) {
                    set_error.set(Some(
                        t_string!(i18n, undercut_alert_err_margin_range).to_string(),
                    ));
                    return;
                }
                AlertTrigger::RetainerUndercut { margin_percent }
            }
            AlertKind::Sold => AlertTrigger::RetainerSold {},
        };

        // Guest mode never reaches the server: `kind` is locked to
        // `ItemPrice` above, so `trigger` is always `BelowThreshold` here.
        // Persist to the local guest-alerts store instead of POSTing, and
        // skip the endpoint-required check entirely (guests have no
        // endpoints to pick from — alerts fire client-side).
        if !signed_in {
            let AlertTrigger::BelowThreshold {
                item_id,
                world_selector,
                price_threshold,
                hq_only,
            } = trigger
            else {
                return;
            };
            let Some(guest) = guest else {
                return;
            };
            let rule = GuestAlertRule::new(item_id, world_selector, price_threshold, hq_only, None);
            if guest.add(rule) {
                if let Some(t) = toasts {
                    t.success(t_string!(i18n, guest_alert_created_toast).to_string());
                }
                set_visible.set(false);
            } else {
                set_error.set(Some(t_string!(i18n, guest_alert_err_limit).to_string()));
            }
            return;
        }

        let endpoint_ids: Vec<i32> = selected_endpoints.get().into_iter().collect();
        if endpoint_ids.is_empty() {
            set_error.set(Some(
                t_string!(i18n, alert_drawer_err_endpoint_required).to_string(),
            ));
            return;
        }
        let req = CreateAlertRequest {
            trigger,
            delivery: None,
            endpoint_ids,
            cooldown_seconds: None,
        };
        let created_kind = kind.get();
        spawn_local(async move {
            match create_alert(req).await {
                Ok(_) => {
                    if let Some(t) = toasts {
                        t.success(match created_kind {
                            AlertKind::ItemPrice => {
                                t_string!(i18n, alert_drawer_created_toast).to_string()
                            }
                            AlertKind::BelowMedian => {
                                t_string!(i18n, below_median_alert_created_toast).to_string()
                            }
                            AlertKind::BackInStock => {
                                t_string!(i18n, back_in_stock_alert_created_toast).to_string()
                            }
                            AlertKind::Undercut => {
                                t_string!(i18n, undercut_alert_created_toast).to_string()
                            }
                            AlertKind::Sold => {
                                t_string!(i18n, sold_alert_created_toast).to_string()
                            }
                        });
                    }
                    set_visible.set(false);
                }
                Err(e) => {
                    set_error.set(Some(format!("{e}")));
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
                    set_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    // Fixed at mount (the preset item never changes for the lifetime of one
    // drawer instance), so this is computed once as a plain value rather
    // than a reactive closure.
    let title = match &preset_item {
        Some((_, name)) => format!("{}{}", t_string!(i18n, alert_drawer_title), name),
        None => t_string!(i18n, add_alert_button).to_string(),
    };

    let kind_label = move |target: AlertKind| match target {
        AlertKind::ItemPrice => t_string!(i18n, alert_kind_item_price).to_string(),
        AlertKind::BelowMedian => t_string!(i18n, alert_kind_below_median).to_string(),
        AlertKind::BackInStock => t_string!(i18n, alert_kind_back_in_stock).to_string(),
        AlertKind::Undercut => t_string!(i18n, alert_kind_undercut).to_string(),
        AlertKind::Sold => t_string!(i18n, alert_kind_sold).to_string(),
    };

    // Wrapping buttons with a ~7rem basis: three kinds fill one row, five
    // wrap to 3 + 2 with the second row stretched to the full width.
    let kind_btn = move |target: AlertKind| {
        view! {
            <button
                type="button"
                class="btn-ghost flex-1 basis-28 justify-center text-center"
                class:bg-brand-500=move || kind.get() == target
                on:click=move |_| {
                    kind.set(target);
                    set_error.set(None);
                }
            >
                {move || kind_label(target)}
            </button>
        }
    };

    // Below-median baseline preview: the median(s) the alert would compare
    // against, or a note that there isn't enough history yet. Renders
    // nothing until the stats resolve, and nothing on a fetch error.
    let baseline_preview = move || {
        let hq = hq_only.get();
        baselines.get().flatten().map(|(scope, baselines)| {
            let usable = usable_baselines(&baselines, hq);
            if usable.is_empty() {
                return t_string!(i18n, below_median_alert_baseline_thin).to_string();
            }
            // Only name the quality when it's ambiguous: an any-quality alert
            // that has an HQ median to show.
            let labelled = !hq && usable.iter().any(|b| b.hq);
            let median = usable
                .iter()
                .map(|b| {
                    let value = t_string!(
                        i18n,
                        below_median_alert_baseline_value,
                        price = b.p50_30d.separate_with_commas(),
                        count = b.cleaned_sample_size_30d
                    )
                    .to_string();
                    if !labelled {
                        value
                    } else if b.hq {
                        format!("{} {value}", t_string!(i18n, hq))
                    } else {
                        format!("{} {value}", t_string!(i18n, nq))
                    }
                })
                .collect::<Vec<_>>()
                .join(" · ");
            t_string!(
                i18n,
                below_median_alert_baseline,
                scope = scope,
                median = median
            )
            .to_string()
        })
    };

    view! {
        <Modal set_visible>
            <div class="p-4 space-y-4 w-[28rem] max-w-full max-h-[80vh] overflow-y-auto">
                <h2 class="text-xl font-bold">{title.clone()}</h2>

                <Show when=move || show_kind_picker>
                    <div class="space-y-1">
                        <label class="text-sm font-semibold">{t!(i18n, alert_kind_label)}</label>
                        <div class="flex flex-wrap gap-2">
                            {available_kinds.iter().map(|k| kind_btn(*k)).collect_view()}
                        </div>
                    </div>
                </Show>

                <Show when=move || signed_in && kind.get() == AlertKind::BelowMedian>
                    <p class="text-sm opacity-80">{t!(i18n, below_median_alert_description)}</p>
                </Show>

                <Show when=move || signed_in && kind.get() == AlertKind::BackInStock>
                    <p class="text-sm opacity-80">
                        {move || {
                            t_string!(
                                i18n,
                                back_in_stock_alert_description,
                                minutes = BACK_IN_STOCK_MIN_EMPTY_SECS / 60
                            )
                            .to_string()
                        }}
                    </p>
                </Show>

                <Show when=move || !locked_to_preset_item && signed_in && kind.get() == AlertKind::Undercut>
                    <p class="text-sm opacity-80">{t!(i18n, undercut_alert_description)}</p>
                </Show>

                <Show when=move || !locked_to_preset_item && signed_in && kind.get() == AlertKind::Sold>
                    <p class="text-sm opacity-80">{t!(i18n, sold_alert_description)}</p>
                </Show>

                <Show when=move || kind.get().is_item_scoped()>
                    <div class="space-y-4">
                        <Show when=move || !locked_to_preset_item>
                            <div class="space-y-1">
                                <label class="text-sm font-semibold" for="create-alert-search">{t!(i18n, create_alert_item_label)}</label>
                                {move || match selected_item.get() {
                                    Some((id, name)) => view! {
                                        <div class="flex items-center justify-between gap-2 rounded border border-[color:var(--color-outline)] p-2">
                                            <div class="flex items-center gap-2 min-w-0">
                                                <ItemIcon item_id=id icon_size=IconSize::Small />
                                                <span class="truncate font-medium">{name}</span>
                                            </div>
                                            <button class="btn-ghost text-xs"
                                                on:click=move |_| {
                                                    selected_item.set(None);
                                                    set_search.set("".into());
                                                }>
                                                {t!(i18n, create_alert_change_item)}
                                            </button>
                                        </div>
                                    }.into_any(),
                                    None => view! {
                                        <div class="space-y-1">
                                            <input
                                                id="create-alert-search"
                                                class="input w-full"
                                                placeholder=t_string!(i18n, create_alert_search_placeholder)
                                                prop:value=search
                                                on:input=move |e| set_search.set(event_target_value(&e))
                                            />
                                            {move || {
                                                let results = item_results();
                                                if results.is_empty() {
                                                    view! { <div class="text-xs opacity-60 px-1">""</div> }.into_any()
                                                } else {
                                                    view! {
                                                        <ul class="max-h-48 overflow-y-auto rounded border border-[color:var(--color-outline)] divide-y divide-[color:var(--color-outline)]">
                                                            {results.into_iter().map(|(id, item)| {
                                                                let item_id = id.0;
                                                                let item_name = item.name.as_str().to_string();
                                                                let item_name_for_button = item_name.clone();
                                                                view! {
                                                                    <li>
                                                                        <button
                                                                            type="button"
                                                                            class="flex items-center gap-2 w-full text-left p-2 hover:bg-[color:var(--color-background-panel)]"
                                                                            on:click=move |_| {
                                                                                selected_item.set(Some((item_id, item_name_for_button.clone())));
                                                                            }
                                                                        >
                                                                            <ItemIcon item_id=item_id icon_size=IconSize::Small />
                                                                            <span class="truncate">{item_name}</span>
                                                                        </button>
                                                                    </li>
                                                                }
                                                            }).collect_view()}
                                                        </ul>
                                                    }.into_any()
                                                }
                                            }}
                                        </div>
                                    }.into_any(),
                                }}
                            </div>
                        </Show>

                        <div class="space-y-1">
                            <label class="text-sm font-semibold">{t!(i18n, alert_drawer_world_label)}</label>
                            <WorldPicker
                                current_world=world.into()
                                set_current_world=set_world.into()
                            />
                        </div>

                        <Show when=move || kind.get() == AlertKind::ItemPrice>
                            <div class="space-y-1">
                                <label class="text-sm font-semibold" for="create-alert-threshold">{t!(i18n, alert_drawer_threshold_label)}</label>
                                <input
                                    id="create-alert-threshold"
                                    class="input w-full"
                                    type="number"
                                    min="1"
                                    placeholder=t_string!(i18n, alert_drawer_threshold_placeholder)
                                    prop:value=price_threshold
                                    on:input=move |e| set_price_threshold.set(event_target_value(&e))
                                />
                            </div>
                        </Show>

                        <Show when=move || kind.get() == AlertKind::BelowMedian>
                            <div class="space-y-1">
                                <label class="text-sm font-semibold" for="below-median-alert-percent">
                                    {t!(i18n, below_median_alert_percent_label)}
                                </label>
                                <input
                                    id="below-median-alert-percent"
                                    class="input w-full"
                                    type="number"
                                    step="1"
                                    min=*BELOW_MEDIAN_PERCENT_RANGE.start()
                                    max=*BELOW_MEDIAN_PERCENT_RANGE.end()
                                    aria-invalid=move || (!percent_below_valid()).to_string()
                                    aria-describedby="below-median-alert-percent-error"
                                    prop:value=percent_below
                                    on:input=move |e| set_percent_below.set(event_target_value(&e))
                                />
                                <Show when=move || !percent_below_valid()>
                                    <p id="below-median-alert-percent-error" class="text-xs text-red-500">
                                        {percent_range_error}
                                    </p>
                                </Show>
                            </div>
                        </Show>

                        <label class="flex items-center gap-2">
                            <input
                                type="checkbox"
                                prop:checked=hq_only
                                on:change=move |e| set_hq_only.set(event_target_checked(&e))
                            />
                            <span class="text-sm">{t!(i18n, hq_only)}</span>
                        </label>

                        {move || {
                            (kind.get() == AlertKind::BelowMedian)
                                .then(baseline_preview)
                                .flatten()
                                .map(|text| view! {
                                    <p class="text-xs text-[color:var(--color-text-muted)]">{text}</p>
                                })
                        }}
                    </div>
                </Show>

                <Show when=move || signed_in && kind.get() == AlertKind::Undercut>
                    <div class="space-y-1">
                        <label class="text-sm font-semibold" for="undercut-alert-margin">
                            {t!(i18n, undercut_alert_margin_label)}
                        </label>
                        <input
                            id="undercut-alert-margin"
                            class="input w-full"
                            type="number"
                            min="0"
                            max="200"
                            prop:value=margin
                            on:input=move |e| set_margin.set(event_target_value(&e))
                        />
                    </div>
                </Show>

                <Show
                    when=move || signed_in
                    fallback=move || view! {
                        <p class="text-sm opacity-70">{t!(i18n, guest_alert_saved_on_device_note)}</p>
                    }
                >
                    <EndpointPicker
                        endpoints=endpoints.expect("endpoints resource exists when signed in")
                        selected=selected_endpoints
                    />
                </Show>

                <div class="space-y-1">
                    <label class="text-sm font-semibold">{t!(i18n, alert_drawer_active_heading)}</label>
                    <Show
                        when=move || signed_in
                        fallback=move || {
                            let device_label = t_string!(i18n, guest_alert_device_label).to_string();
                            let delete_aria = t_string!(i18n, alert_rules_aria_delete_alert).to_string();
                            let rows = guest.map(|g| g.rules().get()).unwrap_or_default();
                            if rows.is_empty() {
                                view! {
                                    <p class="text-sm opacity-70">{t!(i18n, alert_drawer_active_empty)}</p>
                                }.into_any()
                            } else {
                                view! {
                                    <ul class="divide-y divide-[color:var(--color-outline)] rounded border border-[color:var(--color-outline)]">
                                        {rows.into_iter().map(|rule| {
                                            let row_id = rule.id.clone();
                                            let description = item_row_description(
                                                rule.item_id,
                                                &t_string!(i18n, alert_drawer_threshold_below, price = rule.price_threshold).to_string(),
                                                rule.world_selector,
                                                rule.hq_only,
                                                i18n,
                                            );
                                            active_alert_row(description, device_label.clone(), delete_aria.clone(), move || {
                                                if let Some(guest) = guest {
                                                    guest.remove(&row_id);
                                                }
                                            })
                                        }).collect_view()}
                                    </ul>
                                }.into_any()
                            }
                        }
                    >
                        <Suspense fallback=move || {
                            view! { <div class="text-sm opacity-70">{t!(i18n, loading)}</div> }
                        }>
                            {move || alerts.and_then(|res| res.get()).map(|r| match r {
                                Ok(rows) => {
                                    let endpoint_list: Vec<Endpoint> = endpoints
                                        .and_then(|res| res.get())
                                        .and_then(|r| r.ok())
                                        .unwrap_or_default();
                                    let rows: Vec<Alert> = rows
                                        .into_iter()
                                        .filter(|a| trigger_matches_kind(&a.trigger, kind.get()))
                                        .collect();
                                    if rows.is_empty() {
                                        view! {
                                            <p class="text-sm opacity-70">{t!(i18n, alert_drawer_active_empty)}</p>
                                        }.into_any()
                                    } else {
                                        view! {
                                            <ul class="divide-y divide-[color:var(--color-outline)] rounded border border-[color:var(--color-outline)]">
                                                {rows.into_iter().map(|a| {
                                                    let id = a.id;
                                                    let description = match &a.trigger {
                                                        AlertTrigger::BelowThreshold { item_id, price_threshold, world_selector, hq_only } => {
                                                            item_row_description(
                                                                *item_id,
                                                                &t_string!(i18n, alert_drawer_threshold_below, price = *price_threshold).to_string(),
                                                                *world_selector,
                                                                *hq_only,
                                                                i18n,
                                                            )
                                                        }
                                                        AlertTrigger::BelowMedian { item_id, percent_below, world_selector, hq_only } => {
                                                            item_row_description(
                                                                *item_id,
                                                                &t_string!(i18n, alerts_below_median_rule, percent = *percent_below).to_string(),
                                                                *world_selector,
                                                                *hq_only,
                                                                i18n,
                                                            )
                                                        }
                                                        AlertTrigger::BackInStock { item_id, world_selector, hq_only } => {
                                                            item_row_description(
                                                                *item_id,
                                                                t_string!(i18n, alert_kind_back_in_stock),
                                                                *world_selector,
                                                                *hq_only,
                                                                i18n,
                                                            )
                                                        }
                                                        AlertTrigger::RetainerUndercut { margin_percent } => {
                                                            format!(
                                                                "{} · {}",
                                                                t_string!(i18n, alerts_retainer_undercut_rule),
                                                                t_string!(i18n, alerts_margin_percent, margin = *margin_percent)
                                                            )
                                                        }
                                                        AlertTrigger::RetainerSold {} => {
                                                            t_string!(i18n, alerts_retainer_sold_rule).to_string()
                                                        }
                                                        // Filtered out above (managed from their list
                                                        // pages). Named rather than `_` so a new trigger
                                                        // has to decide how it renders here.
                                                        AlertTrigger::ListItemThreshold { .. }
                                                        | AlertTrigger::ListUpdate { .. } => String::new(),
                                                    };
                                                    let endpoint_names = a
                                                        .endpoint_ids
                                                        .iter()
                                                        .map(|id| {
                                                            endpoint_list
                                                                .iter()
                                                                .find(|e| e.id == *id)
                                                                .map(|e| e.name.clone())
                                                                .unwrap_or_else(|| format!("#{id}"))
                                                        })
                                                        .collect::<Vec<_>>()
                                                        .join(", ");
                                                    let delete_aria = t_string!(i18n, alert_rules_aria_delete_alert).to_string();
                                                    active_alert_row(description, endpoint_names, delete_aria, move || remove(id))
                                                }).collect_view()}
                                            </ul>
                                        }.into_any()
                                    }
                                }
                                Err(e) => view! {
                                    <div class="text-sm text-red-500">{format!("{e}")}</div>
                                }.into_any(),
                            })}
                        </Suspense>
                    </Show>
                </div>

                <Show when=move || error.get().is_some()>
                    <div class="text-sm text-red-500">{move || error.get().unwrap_or_default()}</div>
                </Show>

                <div class="flex justify-end gap-2 pt-2">
                    <button class="btn-ghost" on:click=move |_| set_visible.set(false)>
                        {t!(i18n, cancel)}
                    </button>
                    <button
                        class="btn disabled:opacity-50 disabled:cursor-not-allowed"
                        on:click=submit
                        prop:disabled=move || {
                            kind.get() == AlertKind::BelowMedian && !percent_below_valid()
                        }
                    >
                        <Icon icon=i::BsBell width="1em" height="1em" />
                        <span class="ml-1">
                            {move || match kind.get() {
                                AlertKind::ItemPrice => t_string!(i18n, alert_drawer_submit).to_string(),
                                AlertKind::BelowMedian => t_string!(i18n, below_median_alert_submit).to_string(),
                                AlertKind::BackInStock => t_string!(i18n, back_in_stock_alert_submit).to_string(),
                                AlertKind::Undercut => t_string!(i18n, undercut_alert_submit).to_string(),
                                AlertKind::Sold => t_string!(i18n, sold_alert_submit).to_string(),
                            }}
                        </span>
                    </button>
                </div>
            </div>
        </Modal>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guest_mode_only_offers_item_price() {
        assert_eq!(kinds_for(false, false), &[AlertKind::ItemPrice]);
        assert_eq!(kinds_for(false, true), &[AlertKind::ItemPrice]);
        assert_eq!(
            kinds_for(true, false),
            &[
                AlertKind::ItemPrice,
                AlertKind::BelowMedian,
                AlertKind::BackInStock,
                AlertKind::Undercut,
                AlertKind::Sold,
            ]
        );
    }

    #[test]
    fn preset_item_offers_only_item_scoped_kinds() {
        let kinds = kinds_for(true, true);
        assert_eq!(
            kinds,
            &[
                AlertKind::ItemPrice,
                AlertKind::BelowMedian,
                AlertKind::BackInStock,
            ]
        );
        assert!(kinds.iter().all(|k| k.is_item_scoped()));
        assert!(!AlertKind::Undercut.is_item_scoped());
        assert!(!AlertKind::Sold.is_item_scoped());
    }

    #[test]
    fn market_trigger_kinds_match_only_their_own_trigger() {
        let below_median = AlertTrigger::BelowMedian {
            item_id: 5,
            world_selector: AnySelector::Datacenter(4),
            percent_below: 30,
            hq_only: false,
        };
        let back_in_stock = AlertTrigger::BackInStock {
            item_id: 5,
            world_selector: AnySelector::World(34),
            hq_only: true,
        };
        assert!(trigger_matches_kind(&below_median, AlertKind::BelowMedian));
        assert!(!trigger_matches_kind(&below_median, AlertKind::ItemPrice));
        assert!(!trigger_matches_kind(&below_median, AlertKind::BackInStock));
        assert!(trigger_matches_kind(&back_in_stock, AlertKind::BackInStock));
        assert!(!trigger_matches_kind(&back_in_stock, AlertKind::ItemPrice));
        assert!(!trigger_matches_kind(
            &back_in_stock,
            AlertKind::BelowMedian
        ));
    }

    #[test]
    fn percent_below_accepts_only_the_server_range() {
        assert_eq!(parse_percent_below("30"), Some(30));
        assert_eq!(parse_percent_below(" 5 "), Some(5));
        assert_eq!(parse_percent_below("90"), Some(90));
        assert_eq!(parse_percent_below("4"), None);
        assert_eq!(parse_percent_below("91"), None);
        assert_eq!(parse_percent_below("12.5"), None);
        assert_eq!(parse_percent_below(""), None);
        assert!(BELOW_MEDIAN_PERCENT_RANGE.contains(&DEFAULT_PERCENT_BELOW));
    }

    fn variant(hq: bool, p50: u32, cleaned: u32) -> ItemStatsVariant {
        ItemStatsVariant {
            hq,
            sample_size_30d: cleaned,
            cleaned_sample_size_30d: cleaned,
            vwap_30d: p50,
            p50_30d: p50,
            confidence_band: ultros_api_types::trends::ConfidenceBand::High,
            launder_suspicion: 0.0,
        }
    }

    #[test]
    fn usable_baselines_follow_the_hq_toggle() {
        let both = QualityBaselines::from_variants([
            variant(true, 20_000, 31),
            variant(false, 12_400, 86),
        ]);
        let any: Vec<_> = usable_baselines(&both, false)
            .iter()
            .map(|b| b.hq)
            .collect();
        assert_eq!(any, vec![false, true], "NQ first, then HQ");
        let hq: Vec<_> = usable_baselines(&both, true).iter().map(|b| b.hq).collect();
        assert_eq!(hq, vec![true]);
    }

    #[test]
    fn usable_baselines_drop_thin_history() {
        let thin =
            QualityBaselines::from_variants([variant(false, 12_400, 3), variant(true, 20_000, 31)]);
        let any: Vec<_> = usable_baselines(&thin, false)
            .iter()
            .map(|b| b.hq)
            .collect();
        assert_eq!(any, vec![true]);
        let nq_only = QualityBaselines::from_variants([variant(false, 12_400, 3)]);
        assert!(usable_baselines(&nq_only, false).is_empty());
        assert!(usable_baselines(&nq_only, true).is_empty());
    }

    #[test]
    fn item_price_kind_matches_only_below_threshold() {
        let below = AlertTrigger::BelowThreshold {
            item_id: 5,
            world_selector: AnySelector::World(34),
            price_threshold: 1000,
            hq_only: false,
        };
        assert!(trigger_matches_kind(&below, AlertKind::ItemPrice));
        assert!(!trigger_matches_kind(&below, AlertKind::Undercut));
    }

    #[test]
    fn undercut_kind_matches_only_retainer_undercut() {
        let undercut = AlertTrigger::RetainerUndercut { margin_percent: 5 };
        assert!(trigger_matches_kind(&undercut, AlertKind::Undercut));
        assert!(!trigger_matches_kind(&undercut, AlertKind::ItemPrice));
    }

    #[test]
    fn sold_kind_matches_only_retainer_sold() {
        let sold = AlertTrigger::RetainerSold {};
        assert!(trigger_matches_kind(&sold, AlertKind::Sold));
        assert!(!trigger_matches_kind(&sold, AlertKind::Undercut));
        assert!(!trigger_matches_kind(&sold, AlertKind::ItemPrice));
        let undercut = AlertTrigger::RetainerUndercut { margin_percent: 5 };
        assert!(!trigger_matches_kind(&undercut, AlertKind::Sold));
    }

    #[test]
    fn list_scoped_alerts_never_show_in_drawer() {
        for trigger in [
            AlertTrigger::ListItemThreshold { list_id: 1 },
            AlertTrigger::ListUpdate { list_id: 1 },
        ] {
            assert!(!trigger_matches_kind(&trigger, AlertKind::ItemPrice));
            assert!(!trigger_matches_kind(&trigger, AlertKind::Undercut));
            assert!(!trigger_matches_kind(&trigger, AlertKind::Sold));
            assert!(!trigger_matches_kind(&trigger, AlertKind::BelowMedian));
            assert!(!trigger_matches_kind(&trigger, AlertKind::BackInStock));
        }
    }
}
