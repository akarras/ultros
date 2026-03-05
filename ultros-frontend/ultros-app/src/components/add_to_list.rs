use crate::components::icon::Icon;
use icondata as i;
use icondata::RiPlayListAddMediaLine;
use leptos::component;
use leptos::either::Either;
use leptos::either::EitherOf3;
use leptos::prelude::*;
use leptos::reactive::wrappers::write::SignalSetter;
use leptos::task::spawn_local;
use ultros_api_types::icon_size::IconSize;
use ultros_api_types::list::ListItem;
use xiv_gen::ItemId;

use crate::api::add_item_to_list;
use crate::api::get_lists;
use crate::components::toggle::Toggle;
use crate::components::tooltip::Tooltip;
use crate::components::{item_icon::ItemIcon, loading::Loading, modal::Modal};
use crate::global_state::toasts::use_toast;
use crate::i18n::*;

#[component]
pub fn AddToList(
    #[prop(into)] item_id: Signal<i32>,
    #[prop(optional, into)] class: Option<String>,
) -> impl IntoView {
    let i18n = use_i18n();
    let (modal_visible, set_modal_visible) = signal(false);
    let class = class.unwrap_or("btn-primary".to_string());
    view! {
        <Tooltip tooltip_text=t_string!(i18n, add_to_list_tooltip).to_string()>
            <button
                class=class.clone()
                attr:aria-label=t_string!(i18n, add_to_list_aria_label).to_string()
                on:click=move |_| {
                    set_modal_visible(!modal_visible());
                }
            >
                <Icon icon=RiPlayListAddMediaLine />
                <div class="sr-only">{t!(i18n, add_to_list_sr_only)}</div>
                <Show when=modal_visible>
                    <AddToListModal item_id set_visible=set_modal_visible />
                </Show>
            </button>
        </Tooltip>
    }
}

#[component]
fn AddToListModal(
    item_id: Signal<i32>,
    #[prop(into)] set_visible: SignalSetter<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let items = &xiv_gen_db::data().items;
    let item = move || items.get(&ItemId(item_id()));
    let lists = Resource::new(move || {}, move |_| get_lists());
    let (hq, set_hq) = signal(false);
    let (quantity, set_quantity) = signal(1);
    let quantity_id = move || format!("add-to-list-qty-{}", item_id());

    view! {
        <Modal set_visible>
            <div class="panel p-6 rounded-xl space-y-4">
                <div class="flex items-start gap-3">
                    <div class="shrink-0">
                        <ItemIcon item_id icon_size=IconSize::Medium />
                    </div>
                    <div class="min-w-0 flex-1">
                        <div class="text-xl font-extrabold text-[color:var(--brand-fg)]">{t!(i18n, add_to_list_title)}</div>
                        <div class="text-[color:var(--color-text-muted)] truncate">
                            {move || item().map(|i| i.name.to_string()).unwrap_or_else(|| t_string!(i18n, add_to_list_unknown_item).to_string())}
                        </div>
                    </div>
                    <button class="btn-secondary" on:click=move |_| set_visible(false)>{t!(i18n, add_to_list_close)}</button>
                </div>

                <div class="flex flex-wrap items-center gap-3">
                    <label
                        class="text-sm text-[color:var(--color-text-muted)]"
                        for=quantity_id
                    >
                        {t!(i18n, add_to_list_quantity)}
                    </label>
                    <input
                        id=quantity_id
                        type="number"
                        min="1"
                        class="input w-24"
                        prop:value=quantity
                        on:input=move |e| {
                            let Ok(q) = event_target_value(&e).parse::<i32>() else { return; };
                            set_quantity(q.max(1));
                        }
                    />
                    <div class="h-6 w-px bg-[color:var(--color-outline)] mx-1"></div>
                    <Toggle
                        checked=hq
                        set_checked=set_hq
                        checked_label=t_string!(i18n, add_to_list_hq).to_string()
                        unchecked_label=t_string!(i18n, add_to_list_normal_quality).to_string()
                    />
                </div>

                <div class="rounded p-1">
                    <Suspense fallback=Loading>
                        {move || {
                            let Ok(lists) = lists.get()? else {
                                return Some(Either::Right(view! {
                                    <div class="text-red-400 text-sm">{t!(i18n, add_to_list_unable_to_load)}</div>
                                }));
                            };

                            Some(Either::Left(
                                lists
                                    .into_iter()
                                    .map(|list| {
                                        let (saved, set_saved) = signal(false);
                                        let (running, set_running) = signal(false);
                                        let (error, set_error) = signal(Option::<String>::None);
                                        let toasts = use_toast();
                                        let list_name = list.name.clone();

                                        view! {
                                            <div class="space-y-1">
                                                <div class="flex items-center justify-between card p-2">
                                                    <div class="font-semibold truncate">{list.name}</div>
                                                    <button
                                                        class="btn-primary"
                                                        aria-label=move || format!("{} {}", t_string!(i18n, add_to_list_aria_label_list), list_name)
                                                        disabled=running
                                                        on:click=move |_| {
                                                            set_error(None);
                                                            set_running(true);
                                                            let list_id = list.id;
                                                            let item_id_val = item_id.get_untracked();
                                                            let is_hq = hq.get_untracked();
                                                            let qty = quantity.get_untracked().max(1);
                                                            spawn_local(async move {
                                                                let res = add_item_to_list(
                                                                    list_id,
                                                                    ListItem {
                                                                        id: 0,
                                                                        item_id: item_id_val,
                                                                        list_id,
                                                                        hq: Some(is_hq),
                                                                        quantity: Some(qty),
                                                                        acquired: None,
                                                                    },
                                                                ).await;
                                                                match res {
                                                                    Ok(()) => {
                                                                        set_saved(true);
                                                                        if let Some(toasts) = toasts {
                                                                            toasts.success(t_string!(i18n, add_to_list_success_toast));
                                                                        }
                                                                    }
                                                                    Err(e) => {
                                                                        set_error(Some(format!("{e}")));
                                                                        if let Some(toasts) = toasts {
                                                                            toasts.error(format!("{} {e}", t_string!(i18n, add_to_list_error_toast)));
                                                                        }
                                                                    }
                                                                }
                                                                set_running(false);
                                                            });
                                                        }
                                                    >
                                                        {move || {
                                                            if saved() {
                                                                EitherOf3::A(view! { <span>{t!(i18n, add_to_list_added_success)}</span> })
                                                            } else if running() {
                                                                EitherOf3::B(view! { <span>{t!(i18n, add_to_list_adding)}</span> })
                                                            } else {
                                                                EitherOf3::C(view! {
                                                                    <div class="flex items-center gap-1">
                                                                        <Icon
                                                                            attr:class="text-[color:var(--color-text)]"
                                                                            icon=i::BiPlusRegular
                                                                            width="1.1em"
                                                                            height="1.1em"
                                                                        />
                                                                        <span>{t!(i18n, add_to_list_add)}</span>
                                                                    </div>
                                                                })
                                                            }
                                                        }}
                                                    </button>
                                                </div>
                                                <Show when=Signal::derive(move || error().is_some())>
                                                    <div class="text-xs text-red-400 px-2">
                                                        {move || error().unwrap_or_default()}
                                                    </div>
                                                </Show>
                                            </div>
                                        }
                                    })
                                    .collect::<Vec<_>>()
                                    .into_view(),
                            ))
                        }}
                    </Suspense>
                </div>
            </div>
        </Modal>
    }
}
