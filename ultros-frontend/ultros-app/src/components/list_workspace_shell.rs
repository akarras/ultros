//! The frame around a Lists 2.0 editor, shared by account lists
//! (`routes::list_view_sync`) and device lists (`routes::guest_lists`).
//!
//! Layout only. The name, status, actions, scope and travel state are all
//! passed in, so each route keeps its own document, sync and storage
//! lifecycles and the two pages cannot drift apart visually again. Top to
//! bottom: a back link, the inline-editable name with one status line and
//! the page's actions (one primary action plus a ⋮ menu), the caller's
//! notices, the Build / Shop toggle, one price row (scope picker, optional
//! refresh, travel limit) and then whatever the caller renders below.

use crate::components::icon::Icon;
use crate::components::list_travel_state::{ListTravelPanel, ListTravelState};
use crate::components::modal::Modal;
use crate::components::world_picker::WorldPicker;
use crate::i18n::*;
use crate::routes::list_view_sync::ListWorkspaceModes;
use leptos::prelude::*;
use leptos::reactive::wrappers::write::SignalSetter;
use ultros_api_types::world_helper::AnySelector;

#[component]
pub fn ListWorkspaceShell(
    /// The list's committed name; the input re-syncs to it on every change.
    #[prop(into)]
    name: Signal<String>,
    /// Whether the name input accepts edits. Read-only, it still renders
    /// the same way so both pages keep the same title.
    #[prop(into)]
    can_rename: Signal<bool>,
    /// Called with the raw input value on `change`; the caller trims and
    /// validates, and reports failures through its own feedback.
    #[prop(into)]
    on_rename: Callback<String>,
    /// One line under the name: where the document stands.
    #[prop(into)]
    status: Signal<String>,
    status_testid: &'static str,
    /// Optional second clause after the status (the account page's live
    /// connection label). Rendered after a separator when `status` is set.
    #[prop(optional, into)]
    status_detail: Option<Signal<String>>,
    /// Buttons that belong to the status (retry a failed save, download a
    /// recovery copy). Rendered inline after the text.
    #[prop(optional, into)]
    status_actions: Option<ViewFn>,
    /// The page's one primary action, left of the ⋮ menu button.
    #[prop(optional, into)]
    primary: Option<ViewFn>,
    menu_testid: &'static str,
    /// Owned by the caller so it can close the menu from inside and count
    /// it as an open dialog for the undo keybindings.
    menu_open: RwSignal<bool>,
    /// The ⋮ menu's body, rendered inside a modal titled "More options".
    #[prop(into)]
    menu: ViewFn,
    /// Errors and hints, between the header and the mode toggle.
    #[prop(optional, into)]
    notices: Option<ViewFn>,
    /// False hides the mode toggle and the price row (the device page's
    /// recovery mode). The header and children stay.
    #[prop(into)]
    show_controls: Signal<bool>,
    #[prop(into)] shop: Signal<bool>,
    #[prop(into)] set_shop: Callback<bool>,
    #[prop(into)] scope: Signal<Option<AnySelector>>,
    #[prop(into)] set_scope: Callback<Option<AnySelector>>,
    /// False hides the scope picker only; the travel panel stays.
    #[prop(into)]
    can_set_scope: Signal<bool>,
    /// An explicit price refresh control, for a page without live prices.
    #[prop(optional, into)]
    refresh: Option<ViewFn>,
    price_row_testid: &'static str,
    travel: ListTravelState,
    children: Children,
) -> impl IntoView {
    let i18n = use_i18n();
    // Read from inside two nested `Fn` closures (the `Show` and the modal's
    // children), which a moved `ViewFn` would turn into `FnOnce`.
    let menu = StoredValue::new(menu);
    let status_detail_text = move || {
        let Some(detail) = status_detail else {
            return String::new();
        };
        let detail = detail.get();
        if detail.is_empty() {
            String::new()
        } else if status.with(String::is_empty) {
            detail
        } else {
            format!("· {detail}")
        }
    };
    view! {
        <div class="space-y-3" data-testid="list-workspace-shell">
            <a class="inline-block text-sm text-[color:var(--color-text-muted)] hover:underline" href="/list?labs=lists-sync">{t!(i18n, guest_workspace_back)}</a>
            <header class="flex flex-wrap items-center justify-between gap-x-4 gap-y-2">
                <div class="min-w-0 flex-1 basis-56">
                    <input
                        class="w-full min-w-0 bg-transparent text-2xl font-bold rounded-md border border-transparent hover:border-[color:var(--color-outline)] focus:border-[color:var(--color-outline)] read-only:hover:border-transparent read-only:focus:border-transparent px-1 py-0.5"
                        data-testid="list-name-input"
                        aria-label=move || t_string!(i18n, guest_workspace_name).to_string()
                        readonly=move || !can_rename.get()
                        prop:value=move || name.get()
                        data-committed=move || name.get()
                        maxlength="100"
                        on:keydown=move |ev| {
                            if ev.key() == "Escape" {
                                event_target::<web_sys::HtmlInputElement>(&ev).set_value(&name.get_untracked());
                                ev.stop_propagation();
                            } else if ev.key() == "Enter" {
                                let _ = event_target::<web_sys::HtmlInputElement>(&ev).blur();
                            }
                        }
                        on:change=move |ev| {
                            if can_rename.get_untracked() {
                                on_rename.run(event_target_value(&ev));
                            }
                        }
                    />
                    <p class="flex flex-wrap items-center gap-x-2 gap-y-1 px-1 text-xs text-[color:var(--color-text-muted)]">
                        <span data-testid=status_testid role="status" aria-live="polite">{move || status.get()}</span>
                        <span>{status_detail_text}</span>
                        {status_actions.map(|actions| actions.run())}
                    </p>
                </div>
                <div class="flex flex-wrap items-center gap-2">
                    {primary.map(|primary| primary.run())}
                    <button
                        type="button"
                        class="btn-secondary inline-flex min-h-11 min-w-11 items-center justify-center !px-2"
                        data-testid=menu_testid
                        aria-label=move || t_string!(i18n, online_more).to_string()
                        title=move || t_string!(i18n, online_more).to_string()
                        aria-haspopup="dialog"
                        on:click=move |_| menu_open.set(true)
                    >
                        <Icon icon=icondata::BsThreeDotsVertical aria_hidden=true />
                    </button>
                </div>
            </header>
            {notices.map(|notices| notices.run())}
            <Show when=move || show_controls.get()>
                <ListWorkspaceModes shop set_shop />
                <div class="flex flex-wrap items-center gap-2" data-testid=price_row_testid>
                    <Show when=move || can_set_scope.get()>
                        <WorldPicker current_world=scope set_current_world=SignalSetter::map(move |value| set_scope.run(value)) />
                    </Show>
                    {refresh.clone().map(|refresh| refresh.run())}
                    <ListTravelPanel state=travel />
                </div>
            </Show>
            {children()}
            <Show when=move || menu_open.get()>
                <Modal set_visible=menu_open.write_only() aria_label=Signal::derive(move || t_string!(i18n, online_more).to_string())>
                    <h2 class="text-xl font-bold">{t!(i18n, online_more)}</h2>
                    <div class="space-y-3 pt-3">{menu.get_value().run()}</div>
                </Modal>
            </Show>
        </div>
    }
}
