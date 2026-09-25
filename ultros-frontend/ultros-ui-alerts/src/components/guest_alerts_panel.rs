//! `/alerts` guest (no-account) view.
//!
//! A visitor without an account still gets a full alerts surface here: create
//! client-side price-alert rules (via [`AlertDrawer`] in guest mode), see
//! them in a small table with an enable/disable toggle and delete, opt this
//! browser tab into OS-level `Notification`s, and see the local half of the
//! notification inbox (hits that fired while this tab was open). A sign-in
//! CTA closes the loop — once they sign in, [`GuestAlertAdoptionBanner`]
//! (mounted on the signed-in `/alerts` view) offers to copy these rules onto
//! the account.
//!
//! SSR/CSR parity: this renders the *exact same* [`ActionableEmptyState`]
//! markup `/alerts` has always shown a signed-out visitor, until a
//! post-hydration `Effect` flips `hydrated` — the same idiom
//! [`GuestAlertAdoptionBanner`][crate::components::guest_alert_adoption::GuestAlertAdoptionBanner]
//! and the `Inbox`/`GuestAlerts` stores use. The server and the first client
//! render both see `hydrated == false` and produce byte-identical output; the
//! real guest UI only appears once an `Effect` (client-only, post-render) has
//! run.

use icondata as i;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_i18n::I18nContext;
use ultros_api_types::world_helper::AnySelector;
use ultros_ui::components::relative_time::RelativeToNow;
use ultros_ui::components::tool_help::ActionableEmptyState;
use xiv_gen::ItemId;

use crate::components::alert_drawer::AlertDrawer;
use crate::components::icon::Icon;
use crate::global_state::guest_alerts::{GuestAlertRule, use_guest_alerts};
use crate::global_state::local_world_data::use_world_display_name;
use crate::global_state::notifications::{InboxId, use_inbox};
use crate::global_state::toasts::use_toast;
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::{Locale, t, t_string, use_i18n};

/// Requests OS-level notification permission for this tab. Mirrors the first
/// two steps of `push_subscribe::enable_browser_notifications` (this is
/// deliberately *not* the full Web Push subscribe flow — a guest has no
/// account to register a server-side push endpoint against; this only grants
/// the permission [`crate::components::inbox_live::maybe_show_browser_notification`]
/// and the guest realtime evaluator already check before showing an OS
/// notification).
///
/// `pub` (not crate-private): the non-hydrate stub below would otherwise be
/// unreachable dead code on the `ssr`-only build, same reasoning as
/// `enable_browser_notifications`.
///
/// `None` when the Notification API is unavailable or the request itself
/// failed (as opposed to the user answering it) — the caller shows nothing
/// in that case.
#[cfg(all(feature = "hydrate", target_arch = "wasm32"))]
pub async fn request_notification_permission() -> Option<bool> {
    use wasm_bindgen_futures::JsFuture;
    use web_sys::Notification;

    let promise = Notification::request_permission().ok()?;
    let result = JsFuture::from(promise).await.ok()?;
    Some(result.as_string().as_deref() == Some("granted"))
}

#[cfg(not(all(feature = "hydrate", target_arch = "wasm32")))]
pub async fn request_notification_permission() -> Option<bool> {
    None
}

/// Display strings for one [`GuestAlertRule`]'s row: item name, price
/// threshold, world/datacenter/region and the HQ-only marker — the same four
/// fields `alert_drawer`'s active-list row shows, just kept as separate
/// columns here instead of one formatted description.
fn guest_rule_display(
    i18n: I18nContext<Locale, crate::i18n::I18nKeys>,
    rule: &GuestAlertRule,
) -> (String, String, String, String) {
    let item_name = tracked_data()
        .items
        .get(&ItemId(rule.item_id))
        .map(|it| it.name.as_str().to_string())
        .unwrap_or_else(|| {
            t_string!(i18n, lists_workspace_item_fallback, id = rule.item_id).to_string()
        });
    let threshold_str = t_string!(
        i18n,
        alert_drawer_threshold_below,
        price = rule.price_threshold
    )
    .to_string();
    let world_str = use_world_display_name(rule.world_selector).unwrap_or_else(|| {
        let (AnySelector::World(id) | AnySelector::Datacenter(id) | AnySelector::Region(id)) =
            rule.world_selector;
        format!("#{id}")
    });
    let hq_str = if rule.hq_only {
        t_string!(i18n, alerts_hq_any).to_string()
    } else {
        t_string!(i18n, alerts_any).to_string()
    };
    (item_name, threshold_str, world_str, hq_str)
}

#[component]
pub fn GuestAlertsView() -> impl IntoView {
    let i18n = use_i18n();
    let guest = use_guest_alerts();
    let inbox = use_inbox();
    let toasts = use_toast();

    let hydrated = RwSignal::new(false);
    let browser_notif_granted = RwSignal::new(false);
    Effect::new(move |_| {
        hydrated.set(true);
        #[cfg(all(feature = "hydrate", target_arch = "wasm32"))]
        {
            use web_sys::{Notification, NotificationPermission};
            browser_notif_granted
                .set(Notification::permission() == NotificationPermission::Granted);
        }
    });

    let (drawer_visible, set_drawer_visible) = signal(false);

    let enable_browser = move |_| {
        spawn_local(async move {
            match request_notification_permission().await {
                Some(true) => {
                    browser_notif_granted.set(true);
                    if let Some(t) = toasts {
                        t.success(t_string!(i18n, guest_alert_browser_enabled_toast).to_string());
                    }
                }
                Some(false) => {
                    if let Some(t) = toasts {
                        t.error(t_string!(i18n, guest_alert_browser_denied_toast).to_string());
                    }
                }
                None => {}
            }
        });
    };

    view! {
        <Show
            when=move || hydrated.get()
            fallback=move || {
                view! {
                    <ActionableEmptyState
                        title=t_string!(i18n, alerts_empty_title).to_string()
                        body=t_string!(i18n, alerts_empty_body).to_string()
                        action_href="/login?next=/alerts"
                        action_label=t_string!(i18n, sign_in_discord).to_string()
                        action_external=true
                    />
                }
            }
        >
            <div class="space-y-6" data-testid="guest-alerts-view">
                <p class="text-sm text-[color:var(--color-text-muted)]">
                    {t!(i18n, guest_alert_intro)}
                </p>

                <div class="flex flex-wrap gap-2 justify-end">
                    <Show when=move || !browser_notif_granted.get()>
                        <button class="btn-secondary" on:click=enable_browser>
                            <Icon icon=i::BsBellFill />
                            <span class="ml-1">{t!(i18n, guest_alert_enable_browser)}</span>
                        </button>
                    </Show>
                    <button class="btn" on:click=move |_| set_drawer_visible.set(true)>
                        <Icon icon=i::BsBell />
                        <span class="ml-1">{t!(i18n, add_alert_button)}</span>
                    </button>
                </div>
                <Show when=move || drawer_visible.get()>
                    <AlertDrawer set_visible=set_drawer_visible.into() />
                </Show>

                <div class="space-y-2">
                    <h2 class="text-lg font-semibold">{t!(i18n, guest_alert_rules_heading)}</h2>
                    {move || {
                        let rules = guest.map(|g| g.rules().get()).unwrap_or_default();
                        if rules.is_empty() {
                            view! {
                                <p class="opacity-70 text-sm">{t!(i18n, guest_alert_rules_empty)}</p>
                            }
                                .into_any()
                        } else {
                            view! {
                                <div class="overflow-x-auto">
                                    <table class="w-full text-sm">
                                        <thead>
                                            <tr>
                                                <th scope="col" class="text-left p-1">{t!(i18n, item)}</th>
                                                <th scope="col" class="text-left p-1">{t!(i18n, alert_rules_col_threshold)}</th>
                                                <th scope="col" class="text-left p-1">{t!(i18n, world)}</th>
                                                <th scope="col" class="text-left p-1">{t!(i18n, hq)}</th>
                                                <th scope="col" class="text-left p-1">{t!(i18n, actions)}</th>
                                            </tr>
                                        </thead>
                                        <tbody>
                                            {rules
                                                .into_iter()
                                                .map(|rule| {
                                                    let (item_name, threshold_str, world_str, hq_str) =
                                                        guest_rule_display(i18n, &rule);
                                                    let enabled = rule.enabled;
                                                    let id_for_toggle = rule.id.clone();
                                                    let id_for_delete = rule.id.clone();
                                                    view! {
                                                        <tr class="border-t">
                                                            <td class="p-1">{item_name}</td>
                                                            <td class="p-1">{threshold_str}</td>
                                                            <td class="p-1">{world_str}</td>
                                                            <td class="p-1">{hq_str}</td>
                                                            <td class="p-1 flex gap-1">
                                                                <button
                                                                    class="btn-ghost"
                                                                    aria-label=t_string!(i18n, alert_rules_aria_toggle_enabled)
                                                                    on:click=move |_| {
                                                                        if let Some(guest) = guest {
                                                                            guest.set_enabled(&id_for_toggle, !enabled);
                                                                        }
                                                                    }
                                                                >
                                                                    <Icon icon=if enabled { i::BsPauseFill } else { i::BsPlayFill } />
                                                                </button>
                                                                <button
                                                                    class="btn-ghost text-negative"
                                                                    aria-label=t_string!(i18n, alert_rules_aria_delete_alert)
                                                                    on:click=move |_| {
                                                                        if let Some(guest) = guest {
                                                                            guest.remove(&id_for_delete);
                                                                        }
                                                                    }
                                                                >
                                                                    <Icon icon=i::BiTrashSolid />
                                                                </button>
                                                            </td>
                                                        </tr>
                                                    }
                                                })
                                                .collect_view()}
                                        </tbody>
                                    </table>
                                </div>
                            }
                                .into_any()
                        }
                    }}
                </div>

                <div class="space-y-2">
                    <div class="flex items-center justify-between gap-2">
                        <h2 class="text-lg font-semibold">{t!(i18n, guest_alert_history_heading)}</h2>
                        <button
                            class="btn-ghost"
                            on:click=move |_| {
                                if let Some(inbox) = inbox {
                                    inbox.mark_all_read();
                                }
                            }
                        >
                            <Icon icon=i::BsCheck2All />
                            <span class="ml-1">{t!(i18n, inbox_mark_all_read)}</span>
                        </button>
                    </div>
                    {move || {
                        let Some(inbox) = inbox else {
                            return ().into_any();
                        };
                        let local_items: Vec<_> = inbox
                            .items()
                            .get()
                            .into_iter()
                            .filter(|item| matches!(item.id, InboxId::Local(_)))
                            .collect();
                        if local_items.is_empty() {
                            view! {
                                <p class="opacity-70 text-sm">{t!(i18n, history_no_fires)}</p>
                            }
                                .into_any()
                        } else {
                            view! {
                                <ul class="divide-y divide-[color:var(--color-outline)] rounded border border-[color:var(--color-outline)]">
                                    {local_items
                                        .into_iter()
                                        .map(|item| {
                                            let row_id = item.id.clone();
                                            let row_id_kb = item.id.clone();
                                            view! {
                                                <li
                                                    class="p-2 flex items-center justify-between gap-2 cursor-pointer"
                                                    class:opacity-60=item.read
                                                    role="button"
                                                    tabindex="0"
                                                    on:click=move |_| inbox.mark_read(vec![row_id.clone()])
                                                    on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                                                        if ev.key() == "Enter" || ev.key() == " " {
                                                            ev.prevent_default();
                                                            inbox.mark_read(vec![row_id_kb.clone()]);
                                                        }
                                                    }
                                                >
                                                    <div class="min-w-0">
                                                        <div class="text-sm truncate">{item.title.clone()}</div>
                                                        <div class="text-xs opacity-60 truncate">{item.body.clone()}</div>
                                                    </div>
                                                    <span class="text-xs opacity-60 whitespace-nowrap">
                                                        <RelativeToNow timestamp=item.at.naive_utc() />
                                                    </span>
                                                </li>
                                            }
                                        })
                                        .collect_view()}
                                </ul>
                            }
                                .into_any()
                        }
                    }}
                </div>

                <div class="pt-2 border-t border-[color:var(--color-outline)] flex flex-col items-start gap-2">
                    <p class="text-sm text-[color:var(--color-text-muted)]">
                        {t!(i18n, guest_alert_signin_pitch)}
                    </p>
                    <a rel="external" href="/login?next=/alerts" class="btn-primary">
                        {t!(i18n, sign_in_discord)}
                    </a>
                </div>
            </div>
        </Show>
    }
}
