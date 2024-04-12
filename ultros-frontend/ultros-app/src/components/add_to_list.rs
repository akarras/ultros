use icondata::IoAddOutline;
use icondata::RiPlayListAddMediaLine;
use leptos::*;
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
pub fn AddToList(#[prop(into)] item_id: MaybeSignal<i32>) -> impl IntoView {
    let (modal_visible, set_modal_visible) = create_signal(false);
    view! {
        <Tooltip tooltip_text=Oco::Borrowed("Add to list")>
            <button class="cursor-pointer text-white hover:text-violet-300" on:click=move |_| {
                set_modal_visible(!modal_visible());
            }>
                <Icon  icon=RiPlayListAddMediaLine />
                <div class="sr-only">"Add To List"</div>
                {move || {
                    modal_visible().then(|| {
                        view!{ <AddToListModal item_id set_visible=set_modal_visible />}
                    })
                }}
            </button>
        </Tooltip>
    }
}

#[component]
fn AddToListModal(
    item_id: MaybeSignal<i32>,
    #[prop(into)] set_visible: SignalSetter<bool>,
) -> impl IntoView {
    let items = &xiv_gen_db::data().items;
    let item = move || items.get(&ItemId(item_id()));
    let lists = create_local_resource(move || {}, move |_| get_lists());
    let (hq, set_hq) = create_signal(false);
    let (quantity, set_quantity) = create_signal(1);

    view! {
        <Modal set_visible>
        <div class="flex flex-col">
            <div class="text-xl font-bold">"Add to list"</div>
            <div class="flex flex-row">
                <ItemIcon item_id icon_size=IconSize::Medium />
                <span class="text-xl font-semibold">{move || item().map(|i| i.name.as_str())}</span>
            </div>
            <div class="flex flex-col">
                <span>"Quantity to add:"</span>
                <input prop:value=quantity on:input=move |e| {
                    let Ok(quantity) = event_target_value(&e).parse() else {
                        return;
                    };
                    set_quantity(quantity); }/>
            </div>
            <div class="flex flex-row">
                <Toggle checked=hq set_checked=set_hq checked_label="HQ" unchecked_label="Normal quality" />
            </div>
            <div class="rounded p-1 items-between">
                <Suspense fallback=Loading>
                    {move || {
                        let Ok(lists) = lists.get()? else {
                            return Some(view ! {
                                <div>"Unable to get lists- are you logged in?"</div>
                            }.into_view());
                        };
                        Some(lists.into_iter().map(|list| {
                            let (saved, set_saved) = create_signal(false);
                            let (running, set_running) = create_signal(false);


                            view! {
                                <div class="flex flex-row text-xl justify-between">
                                    <div>{list.name}</div>
                                    <div class="flex flex-row hover:bg-violet-950 bg-fuchsia-950 border border-fuchsia-900 rounded hover:bg-fucshia-950 cursor-pointer p-1"
                                        on:click=move |_| {
                                            set_running(true);
                                            spawn_local(async move {
                                                let _ = add_item_to_list(list.id, ListItem { id: 0, item_id: item_id.get_untracked(), list_id: list.id, hq: Some(hq.get_untracked()), quantity: Some(quantity.get_untracked()), acquired: None }).await;
                                                set_saved(true);
                                            });
                                        }>
                                        {move || {
                                            if saved() {
                                                view! {
                                                    <div class="mx-1">"Saved"</div>
                                                }.into_view()
                                            } else if running() {
                                                view !{
                                                    <div class="mx-1">"Saving"</div>
                                                }.into_view()
                                            } else {
                                                view!{
                                                    <Icon class="text-white" icon=IoAddOutline width="1.2em" height="1.2em" />
                                                    <div class="mx-1">"Add"</div>
                                                }.into_view()
                                            }
                                        }}
                                    </div>
                                </div>
                            }
                        }).collect::<Vec<_>>().into_view()
                        )
                    }}
                </Suspense>
            </div>
        </div>
        </Modal>
    }
}
