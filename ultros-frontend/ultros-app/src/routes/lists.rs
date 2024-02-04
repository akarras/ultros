use icondata as i;
use leptos::*;
use leptos_icons::*;
use leptos_router::*;

use crate::api::{create_list, delete_list, edit_list, get_lists};
use crate::components::ad::Ad;
use crate::components::{loading::*, tooltip::*, world_name::*, world_picker::*};
use crate::global_state::home_world::get_price_zone;
use ultros_api_types::list::{CreateList, List};

#[component]
pub fn EditLists() -> impl IntoView {
    let delete_list = create_action(move |id: &i32| delete_list(*id));
    let edit_list = create_action(move |list: &List| edit_list(list.clone()));
    let create_list = create_action(move |list: &CreateList| create_list(list.clone()));
    let lists = create_resource(
        move || {
            (
                delete_list.version().get(),
                edit_list.version().get(),
                create_list.version().get(),
            )
        },
        move |_| get_lists(),
    );
    let (creating, set_creating) = create_signal(false);
    view! {
    <div class="flex-row">
        <span class="content-title">"Edit Lists"</span>
        <Tooltip tooltip_text=MaybeSignal::Static("Create list".into())>
            <button class="btn" on:click=move |_| set_creating(!creating())><Icon icon=i::BiPlusRegular/></button>
        </Tooltip>
    </div>
    {move || creating().then(|| {
        let (new_list, set_new_list) = create_signal("".to_string());
        let (global, _) = get_price_zone();
        let selector = global().map(|global| global.into());
        let (wdr_filter, set_wdr_filter) = create_signal(selector);

        view!{
            <div class="content-well">
                <label for="list-name">"List name:"</label>
                <input class="w-52" id="list-name" prop:value=new_list on:input=move |input| set_new_list(event_target_value(&input)) />
                <label>"World/Datacenter/Region:"</label>
                <WorldPicker current_world=wdr_filter.into() set_current_world=set_wdr_filter.into() />
                <button prop:disabled=move || wdr_filter().is_none() class="btn" on:click=move |_| {
                if let Some(wdr_filter) = wdr_filter() {
                    let list = CreateList {name: new_list(), wdr_filter};
                    create_list.dispatch(list);
                    set_new_list("".to_string());
                    set_creating(false);
                }
            }><Icon icon=i::BiSaveSolid/></button>
        </div>
    }
    })}
    <div class="content-well">
        <Suspense fallback=move || view!{<Loading />}>
        <>
        {move || lists.get().map(|lists| {
            match lists {
                Ok(lists) => view!{

                    <h3>"Current lists"</h3>
                    <table>
                    <tr><td>"List Name"</td><td>"World"</td></tr>
                        <For each=move || lists.clone()
                            key=move |list| list.id
                            children=move |list| {
                                let (is_edit, set_is_edit) = create_signal(false);
                                let (list, _set_list) = create_signal(list);
                                let (name, set_name) = create_signal(list().name);
                                let (current_world, set_current_world) = create_signal(Some(list().wdr_filter));
                                view!{<tr>
                                    {move || if is_edit() {
                                        view!{<td>
                                                <input class="w-52" prop:value=name on:input=move |input| set_name(event_target_value(&input))/>
                                            </td>
                                            <td>
                                                <WorldPicker current_world=current_world.into() set_current_world=set_current_world.into() />
                                            </td>
                                        }.into_view()
                                    } else {
                                        view!{<td><a href=format!("/list/{}", list().id)>{list().name}</a></td><td><WorldName id=list().wdr_filter/></td>}.into_view()
                                    }}
                                    <td>
                                        {move || if is_edit() {
                                            view!{
                                            <Tooltip tooltip_text=Oco::from("Save changes to this list")>
                                                <button class="btn" on:click=move |_| {
                                                    let mut list = list();
                                                    list.name = name();
                                                    if let Some(world) = current_world() {
                                                        list.wdr_filter = world;
                                                    }
                                                    edit_list.dispatch(list);
                                                } >
                                                    <Icon icon=i::BiSaveSolid/>
                                                </button>
                                            </Tooltip>
                                            <Tooltip tooltip_text=Oco::from("Delete this list")>
                                                <button class="btn" on:click=move |_| delete_list.dispatch(list().id)>
                                                    <Icon icon=i::BiTrashSolid />
                                                </button>
                                            </Tooltip>
                                        }.into_view()
                                        } else {
                                            view!{
                                        <Tooltip tooltip_text=Oco::from("Edit this list")>
                                            <button class="btn" on:click=move |_| set_is_edit(true)>
                                                <Icon icon=i::BsPencilFill />
                                            </button>
                                        </Tooltip>
                                        }.into_view()
                                        }}
                                    </td>
                                </tr>}
                            }
                        />
                    </table>}.into_view(),
                Err(e) => view!{<div>{format!("Error getting listings\n{e}")}</div>}.into_view()
            }
        })}
        </>
        </Suspense>
    </div>}
}

#[component]
pub fn Lists() -> impl IntoView {
    view! {
    <div class="mx-auto">
        <div class="content-nav">
            <A class="btn-secondary flex flex-row" href="/list">
                <Icon height="2em" width="2em" icon=i::AiOrderedListOutlined />
                "Lists"
            </A>
        </div>
        <div class="main-content flex-column">
            <div class="container mx-auto flex flex-col items-start">
            <Ad class="h-10 w-full" />
            <div class="flex flex-col xl:flex-row">
                <div class="flex flex-col grow">
                <Outlet />
                </div>
                <div class="shrink"><Ad class="h-96 xl:h-[50svh] xl:w-32"/></div>
            </div>
        </div>
        </div>
    </div>
    }
}
