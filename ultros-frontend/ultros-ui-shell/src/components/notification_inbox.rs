//! Sidebar notification-inbox row + drop-up panel.
//!
//! Mirrors `AccountMenu`/`HomeWorldMenu`: a `side-nav-account-trigger`
//! button that expands a `side-nav-account-panel` drop-up above it,
//! dismissed by route change / outside click / Escape via
//! [`use_dismissable`]. Reads the [`Inbox`] global-state store — see
//! `ultros-frontend-core/src/global_state/notifications.rs` for the
//! server+local merge and hydration-gating this UI depends on: `items()`
//! and `unread_count()` are guaranteed empty/zero until an `Effect` has run
//! at least once client-side, so the unread pill and the panel body never
//! disagree between the SSR render and the first client render.
//!
//! Mounted directly above `<HomeWorldMenu/>` in `side_nav.rs`, making it the
//! first row of the pinned bottom cluster — see that file for why the
//! cluster survives the collapsed 56px rail while `.side-nav-info` does not.

use crate::components::app_link::AppLink;
use crate::components::dismissable::use_dismissable;
use crate::components::icon::Icon;
use crate::global_state::notifications::{Inbox, InboxItem, use_inbox};
use crate::global_state::user::BootstrapUser;
use crate::i18n::{t, t_string, use_i18n};
use icondata as i;
use leptos::html;
use leptos::prelude::*;
use ultros_ui::components::relative_time::RelativeToNow;
use ultros_ui_alerts::components::guest_alert_adoption::GuestAlertAdoptionBanner;

/// Caps the unread-count pill's label at `"99+"` so a very active inbox
/// never blows out the width of the sidebar row.
pub fn unread_pill_label(n: usize) -> String {
    if n > 99 {
        "99+".to_string()
    } else {
        n.to_string()
    }
}

#[component]
pub fn NotificationInbox() -> impl IntoView {
    let i18n = use_i18n();
    let (open, set_open) = signal(false);
    let root_ref = NodeRef::<html::Div>::new();

    // Route change, click outside, Escape — the shared idiom.
    let token = use_dismissable(root_ref, move || set_open(false));

    // No provider mounted (e.g. a test harness rendering this component in
    // isolation) — render nothing rather than panic. Every real page mounts
    // `provide_inbox()` at the app root.
    let Some(inbox) = use_inbox() else {
        return ().into_any();
    };

    let items = inbox.items();
    let unread = inbox.unread_count();
    let signed_in = matches!(use_context::<BootstrapUser>(), Some(BootstrapUser(Some(_))));

    view! {
        <div class="side-nav-inbox" node_ref=root_ref>
            <button
                class="side-nav-account-trigger"
                aria-haspopup="true"
                aria-expanded=move || if open.get() { "true" } else { "false" }
                aria-label=t_string!(i18n, inbox_aria_label)
                on:click=move |_| {
                    let opening = !open.get_untracked();
                    if opening {
                        token.opening();
                    }
                    set_open.set(opening);
                }
            >
                <Icon icon=i::BsInbox width="1.1em" height="1.1em" aria_hidden=true />
                <span class="side-nav-label ml-2">{t!(i18n, inbox_title)}</span>
                {move || {
                    let n = unread.get();
                    (n > 0)
                        .then(|| {
                            let label = unread_pill_label(n);
                            view! {
                                <span class="side-nav-count" aria-hidden="true">
                                    {label}
                                </span>
                                <span class="sr-only">
                                    {t_string!(i18n, inbox_unread_sr, count = n).to_string()}
                                </span>
                            }
                        })
                }}
            </button>

            <Show when=move || open.get()>
                <div class="side-nav-account-panel side-nav-inbox-panel" tabindex="-1">
                    <div class="menu-row">
                        <span class="menu-item flex-1">{t!(i18n, inbox_title)}</span>
                        <button
                            class="menu-expand"
                            disabled=move || unread.get() == 0
                            aria-label=t_string!(i18n, inbox_mark_all_read)
                            on:click=move |_| inbox.mark_all_read()
                        >
                            <Icon icon=i::BsCheck2All />
                        </button>
                    </div>

                    <GuestAlertAdoptionBanner compact=true />

                    {move || {
                        let list = items.get();
                        if list.is_empty() {
                            view! {
                                <div class="menu-item muted">{t!(i18n, inbox_empty)}</div>
                            }
                                .into_any()
                        } else {
                            list.into_iter()
                                .take(20)
                                .map(|item| inbox_row(item, inbox))
                                .collect::<Vec<_>>()
                                .into_any()
                        }
                    }}

                    <div class="menu-divider"></div>
                    <AppLink href="/alerts" attr:class="menu-item">
                        {t!(i18n, inbox_manage_alerts)}
                    </AppLink>
                    {(!signed_in)
                        .then(|| {
                            view! {
                                <div class="menu-item muted">{t!(i18n, inbox_guest_footer)}</div>
                                <a rel="external" href="/login?next=/alerts" class="menu-item">
                                    <Icon icon=i::BsDiscord width="1.1em" height="1.1em" />
                                    <span class="ml-2">{t!(i18n, sign_in_discord)}</span>
                                </a>
                            }
                        })}
                </div>
            </Show>
        </div>
    }
    .into_any()
}

/// Renders one inbox row. A plain `<a>` (not `AppLink`) when `url` is set —
/// the router's global click handler intercepts same-origin anchor clicks
/// regardless, and a plain tag keeps this free of `AppLink`'s
/// `aria-current` bookkeeping, which makes no sense for a list of distinct
/// notification targets. Falls back to a `<div>` when an item carries no
/// `url` at all. Either way, clicking marks the item read and (for the
/// anchor case) lets navigation proceed.
fn inbox_row(item: InboxItem, inbox: Inbox) -> impl IntoView {
    let InboxItem {
        id,
        title,
        body,
        url,
        at,
        read,
        ..
    } = item;
    let timestamp = at.naive_utc();
    let on_click = move |_| inbox.mark_read(vec![id.clone()]);

    match url {
        Some(href) => view! {
            <a
                href=href
                class="menu-item inbox-item"
                class:inbox-item-unread=!read
                on:click=on_click
            >
                <span class="inbox-item-title">{title}</span>
                <span class="inbox-item-body">{body}</span>
                <span class="inbox-item-time">
                    <RelativeToNow timestamp=timestamp />
                </span>
            </a>
        }
        .into_any(),
        None => view! {
            <div
                class="menu-item inbox-item"
                class:inbox-item-unread=!read
                on:click=on_click
            >
                <span class="inbox-item-title">{title}</span>
                <span class="inbox-item-body">{body}</span>
                <span class="inbox-item-time">
                    <RelativeToNow timestamp=timestamp />
                </span>
            </div>
        }
        .into_any(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unread_pill_label_caps_at_99_plus() {
        assert_eq!(unread_pill_label(0), "0");
        assert_eq!(unread_pill_label(5), "5");
        assert_eq!(unread_pill_label(99), "99");
        assert_eq!(unread_pill_label(100), "99+");
        assert_eq!(unread_pill_label(1000), "99+");
    }
}
