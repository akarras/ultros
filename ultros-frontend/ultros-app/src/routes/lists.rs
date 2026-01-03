use crate::components::icon::Icon;
use icondata as i;
use leptos::either::Either;
use leptos::prelude::*;
use leptos_router::components::{A, Outlet};

use crate::api::{create_list, delete_list, edit_list, get_lists};
use crate::components::ad::Ad;
use crate::components::{loading::*, tooltip::*, world_name::*, world_picker::*};
use crate::global_state::home_world::get_price_zone;
use ultros_api_types::list::{CreateList, List};

#[component]
fn ListCard(
    list: List,
    edit_list: Action<List, Result<(), crate::error::AppError>>,
    delete_list: Action<i32, Result<(), crate::error::AppError>>,
) -> impl IntoView {
    let (is_edit, set_is_edit) = signal(false);
    // Local state for editing
    let (name, set_name) = signal(list.name.clone());
    let (current_world, set_current_world) = signal(Some(list.wdr_filter.clone()));

    let list_clone_cancel = list.clone();
    let cancel_edit = move |_| {
        set_name(list_clone_cancel.name.clone());
        set_current_world(Some(list_clone_cancel.wdr_filter.clone()));
        set_is_edit(false);
    };

    view! {
        <div class="panel p-4 rounded-xl flex flex-col gap-2 h-full justify-between transition-shadow hover:shadow-lg dark:hover:shadow-gray-700/30 relative">
            {move || {
                let list = list.clone();
                if is_edit() {
                    let list_for_save = list.clone();
                    let list_for_delete = list.clone();
                    Either::Left(view! {
                        <div class="flex flex-col gap-3 w-full">
                             <div>
                                <label class="label text-sm font-semibold">"List Name"</label>
                                <input
                                    class="input w-full"
                                    prop:value=name
                                    on:input=move |input| set_name(event_target_value(&input))
                                />
                            </div>
                            <div>
                                <label class="label text-sm font-semibold">"World/Region"</label>
                                <WorldPicker
                                    current_world=current_world.into()
                                    set_current_world=set_current_world.into()
                                />
                            </div>
                            <div class="flex gap-2 justify-end mt-2">
                                <button class="btn-secondary btn-sm" on:click=cancel_edit.clone()>
                                    <Icon icon=i::AiCloseOutlined /> "Cancel"
                                </button>
                                <button
                                    class="btn-primary btn-sm"
                                    on:click=move |_| {
                                        let mut new_list = list_for_save.clone();
                                        new_list.name = name();
                                        if let Some(world) = current_world() {
                                            new_list.wdr_filter = world;
                                        }
                                        edit_list.dispatch(new_list);
                                        set_is_edit(false);
                                    }
                                >
                                    <Icon icon=i::BiSaveSolid /> "Save"
                                </button>
                            </div>
                            <div class="border-t border-gray-600/50 my-2"></div>
                            <div class="flex justify-between items-center">
                                <span class="text-red-400 text-sm font-semibold">"Danger Zone"</span>
                                <Tooltip tooltip_text="Delete this list">
                                    <button
                                        class="btn-danger btn-sm"
                                        on:click=move |_| {
                                            let _ = delete_list.dispatch(list_for_delete.id);
                                        }
                                    >
                                        <Icon icon=i::BiTrashSolid /> "Delete"
                                    </button>
                                </Tooltip>
                            </div>
                        </div>
                    })
                } else {
                    Either::Right(view! {
                        <>
                            <div class="flex justify-between items-start gap-2">
                                <div class="flex flex-col gap-1 overflow-hidden">
                                    <a href=format!("/list/{}", list.id) class="text-xl font-bold hover:underline truncate text-[color:var(--link-color)]">
                                        {move || name()}
                                    </a>
                                    <div class="text-sm text-gray-400 flex items-center gap-1">
                                         <Icon icon=i::BiWorldRegular />
                                         <WorldName id=list.wdr_filter.clone() />
                                    </div>
                                </div>
                                <Tooltip tooltip_text="Edit List">
                                    <button class="btn-ghost btn-sm text-gray-400 hover:text-white" on:click=move |_| set_is_edit(true) aria_label="Edit List">
                                        <Icon icon=i::BsPencilFill />
                                    </button>
                                </Tooltip>
                            </div>
                            <div class="mt-4 flex justify-end">
                                <a href=format!("/list/{}", list.id) class="btn-secondary btn-sm">
                                    "View Items" <Icon icon=i::AiArrowRightOutlined class="ml-1"/>
                                </a>
                            </div>
                        </>
                    })
                }
            }}
        </div>
    }
}

#[component]
pub fn EditLists() -> impl IntoView {
    let delete_list = Action::new(move |id: &i32| delete_list(*id));
    let edit_list = Action::new(move |list: &List| edit_list(list.clone()));
    let create_list = Action::new(move |list: &CreateList| create_list(list.clone()));
    let lists = Resource::new(
        move || {
            (
                delete_list.version().get(),
                edit_list.version().get(),
                create_list.version().get(),
            )
        },
        move |_| get_lists(),
    );
    let (creating, set_creating) = signal(false);
    let (filter, set_filter) = signal(String::new());

    let filtered_lists = Signal::derive(move || {
        let filter_text = filter.get().to_lowercase();
        lists.get().map(|res| {
            res.map(|lists| {
                if filter_text.is_empty() {
                    lists
                } else {
                    lists
                        .into_iter()
                        .filter(|l| l.name.to_lowercase().contains(&filter_text))
                        .collect()
                }
            })
        })
    });

    view! {
        <div class="flex flex-col gap-4">
            <div class="flex items-center gap-2 md:gap-3">
                <A exact=true attr:class="nav-link" href="/list">
                    <Icon height="1.25em" width="1.25em" icon=i::AiOrderedListOutlined />
                    <span>"Lists"</span>
                </A>
            </div>

            <div class="flex flex-col md:flex-row justify-between items-start md:items-center gap-4">
                <h1 class="text-3xl font-bold text-[color:var(--brand-fg)]">"My Lists"</h1>
                 <button class="btn-primary" on:click=move |_| set_creating(!creating())>
                    <Icon icon=if creating() { i::AiCloseOutlined } else { i::BiPlusRegular } />
                    {move || if creating() { "Cancel Creation" } else { "Create New List" }}
                </button>
            </div>

            {move || {
                creating()
                    .then(|| {
                        let (new_list, set_new_list) = signal("".to_string());
                        let (global, _) = get_price_zone();
                        let selector = global().map(|global| global.into());
                        let (wdr_filter, set_wdr_filter) = signal(selector);
                        view! {
                            <div class="panel p-6 rounded-xl animate-fade-in">
                                <h3 class="text-lg font-bold mb-4">"Create New List"</h3>
                                <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                                    <div class="flex flex-col gap-1">
                                        <label for="new-list-name" class="label font-semibold">"List name"</label>
                                        <input
                                            class="input w-full"
                                            id="new-list-name"
                                            placeholder="My Awesome List"
                                            prop:value=new_list
                                            on:input=move |input| set_new_list(event_target_value(&input))
                                        />
                                    </div>
                                    <div class="flex flex-col gap-1">
                                        <label class="label font-semibold">"World/Region"</label>
                                        <WorldPicker
                                            current_world=wdr_filter.into()
                                            set_current_world=set_wdr_filter.into()
                                        />
                                    </div>
                                </div>
                                <div class="flex justify-end mt-4">
                                    <button
                                        prop:disabled=move || wdr_filter().is_none() || new_list().is_empty()
                                        class="btn-primary"
                                        on:click=move |_| {
                                            if let Some(wdr_filter) = wdr_filter() {
                                                let list = CreateList {
                                                    name: new_list(),
                                                    wdr_filter,
                                                };
                                                create_list.dispatch(list);
                                                set_new_list("".to_string());
                                                set_creating(false);
                                            }
                                        }
                                    >
                                        <Icon icon=i::BiSaveSolid /> "Create List"
                                    </button>
                                </div>
                            </div>
                        }
                    })
            }}

            <div class="relative">
                <div class="absolute inset-y-0 left-0 pl-3 flex items-center pointer-events-none">
                     <Icon icon=i::AiSearchOutlined class="text-gray-400"/>
                </div>
                <input
                    class="input w-full pl-10"
                    placeholder="Search your lists..."
                    prop:value=filter
                    on:input=move |ev| set_filter(event_target_value(&ev))
                />
            </div>

            <Suspense fallback=move || view! { <Loading /> }>
                {move || {
                    filtered_lists
                        .get()
                        .map(|lists| {
                            match lists {
                                Ok(lists) => {
                                    if lists.is_empty() {
                                        Either::Left(view! {
                                            <div class="flex flex-col items-center justify-center py-12 text-gray-400">
                                                <Icon icon=i::AiOrderedListOutlined width="4em" height="4em" class="mb-4 opacity-50"/>
                                                <h3 class="text-xl font-semibold">"No lists found"</h3>
                                                <p>"Create a new list to get started!"</p>
                                            </div>
                                        }.into_any())
                                    } else {
                                        Either::Right(view! {
                                            <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                                                <For
                                                    each=move || lists.clone()
                                                    key=move |list| list.id
                                                    children=move |list| {
                                                        view! {
                                                            <ListCard
                                                                list=list
                                                                edit_list=edit_list
                                                                delete_list=delete_list
                                                            />
                                                        }
                                                    }
                                                />
                                            </div>
                                        }.into_any())
                                    }
                                }
                                Err(e) => {
                                    Either::Right(view! {
                                        <div class="alert alert-error">
                                            {format!("Error loading lists: {e}")}
                                        </div>
                                    }.into_any())
                                }
                            }
                        })
                }}
            </Suspense>
        </div>
    }.into_any()
}

#[component]
pub fn Lists() -> impl IntoView {
    view! {
        <div class="mx-auto">
            <div class="main-content">
                <div class="container mx-auto flex flex-col xl:flex-row items-start gap-4">
                    <div class="flex flex-col grow w-full">
                         <div class="w-full mb-4">
                            <Ad class="h-20 w-full" />
                        </div>
                        <Outlet />
                    </div>
                    <div class="shrink-0 xl:w-80">
                         <Ad class="h-96 w-96 xl:h-[600px] xl:w-80" />
                    </div>
                </div>
            </div>
        </div>
    }
    .into_any()
}
