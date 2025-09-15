use icondata as i;
use icondata::RiPlayListAddMediaLine;
use leptos::component;
use leptos::either::Either;
use leptos::either::EitherOf3;
use leptos::prelude::*;
use leptos::reactive::wrappers::write::SignalSetter;
use leptos::task::spawn_local;
use leptos_icons::*;
use ultros_api_types::icon_size::IconSize;
use ultros_api_types::list::ListItem;
use xiv_gen::ItemId;

use crate::api::add_item_to_list;
use crate::api::get_lists;
use crate::components::toggle::Toggle;
use crate::components::tooltip::Tooltip;
use crate::components::{item_icon::ItemIcon, loading::Loading, modal::Modal};

#[component]
pub fn AddToList(#[prop(into)] item_id: Signal<i32>) -> impl IntoView {
    let (modal_visible, set_modal_visible) = signal(false);
    view! {
        <Tooltip tooltip_text="Add to list">
            <button
                class="btn-primary"
                on:click=move |_| {
                    set_modal_visible(!modal_visible());
                }
            >

                <Icon icon=RiPlayListAddMediaLine />
                <div class="sr-only">"Add To List"</div>
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
    let items = &xiv_gen_db::data().items;
    let item = move || items.get(&ItemId(item_id()));
    let lists = Resource::new(move || {}, move |_| get_lists());
    let (hq, set_hq) = signal(false);
    let (quantity, set_quantity) = signal(1);

    view! {
        <Modal set_visible>
            <div class="flex flex-col">
                <div class="text-xl font-bold">"Add to list"</div>
                <div class="flex flex-row">
                    <ItemIcon item_id icon_size=IconSize::Medium />
                    <span class="text-xl font-semibold">
                        {move || item().map(|i| i.name.as_str())}
                    </span>
                </div>
                <div class="flex flex-col">
                    <span>"Quantity to add:"</span>
                    <input
                        prop:value=quantity
                        on:input=move |e| {
                            let Ok(quantity) = event_target_value(&e).parse() else {
                                return;
                            };
                            set_quantity(quantity);
                        }
                    />

                </div>
                <div class="flex flex-row">
                    <Toggle
                        checked=hq
                        set_checked=set_hq
                        checked_label="HQ"
                        unchecked_label="Normal quality"
                    />
                </div>
                <div class="rounded p-1 items-between">
                    <Suspense fallback=Loading>
                        {move || {
                            let Ok(lists) = lists.get()? else {
                                return Some(
                                    Either::Right(
                                        view! {
                                            <div>"Unable to get lists- are you logged in?"</div>
                                        },
                                    ),
                                );
                            };
                            Some(
                                Either::Left(
                                    lists
                                        .into_iter()
                                        .map(|list| {
                                            let (saved, set_saved) = signal(false);
                                            let (running, set_running) = signal(false);
                                            view! {
                                                <div class="flex flex-row text-xl justify-between">
                                                    <div>{list.name}</div>
                                                    <div
                                                        class="flex flex-row bg-black/30 hover:bg-white/10 border border-white/10 hover:border-white/20 rounded cursor-pointer p-1 transition-colors duration-150 ease-in-out"
                                                        on:click=move |_| {
                                                            set_running(true);
                                                            spawn_local(async move {
                                                                let _ = add_item_to_list(
                                                                        list.id,
                                                                        ListItem {
                                                                            id: 0,
                                                                            item_id: item_id.get_untracked(),
                                                                            list_id: list.id,
                                                                            hq: Some(hq.get_untracked()),
                                                                            quantity: Some(quantity.get_untracked()),
                                                                            acquired: None,
                                                                        },
                                                                    )
                                                                    .await;
                                                                set_saved(true);
                                                            });
                                                        }
                                                    >
                                                        {move || {
                                                            if saved() {
                                                                EitherOf3::A(view! { <div class="mx-1">"Saved"</div> })
                                                            } else if running() {
                                                                EitherOf3::B(view! { <div class="mx-1">"Saving"</div> })
                                                            } else {
                                                                EitherOf3::C(
                                                                    view! {
                                                                        <Icon
                                                                            attr:class="text-gray-200"
                                                                            icon=i::BiPlusRegular
                                                                            width="1.2em"
                                                                            height="1.2em"
                                                                        />
                                                                        <div class="mx-1">"Add"</div>
                                                                    },
                                                                )
                                                            }
                                                        }}
                                                    </div>
                                                </div>
                                            }
                                        })
                                        .collect::<Vec<_>>()
                                        .into_view(),
                                ),
                            )
                        }}

                    </Suspense>
                </div>
            </div>
        </Modal>
    }
}
