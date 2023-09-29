use std::cmp::Reverse;

use crate::api::{get_retainer_listings, get_retainer_undercuts};
use crate::components::ad::Ad;
use crate::components::gil::*;
use crate::components::{item_icon::*, loading::*, meta::*, world_name::*};
use leptos::*;
use leptos_icons::*;
use leptos_router::*;
use ultros_api_types::icon_size::IconSize;
use ultros_api_types::{world_helper::AnySelector, ActiveListing, FfxivCharacter, Retainer};
use xiv_gen::ItemId;

#[component]
fn RetainerTable(retainer: Retainer, listings: Vec<ActiveListing>) -> impl IntoView {
    let data = xiv_gen_db::data();
    let items = &data.items;
    let categories = &data.item_search_categorys;
    let mut listings = listings;
    listings.sort_by_key(|listing| {
        items.get(&ItemId(listing.item_id)).and_then(|item| {
            categories
                .get(&item.item_search_category)
                .map(|category| (category.order, Reverse(item.level_item.0)))
        })
    });
    let listings: Vec<_> = listings
        .into_iter()
        .map(|listing| {
            let item = items.get(&ItemId(listing.item_id));
            let total = listing.quantity * listing.price_per_unit;
            view! {
                <tr>
                    <td>
                        {listing
                            .hq
                            .then_some("HQ")}
                    </td>
                    <td class="flex flex-row">
                        {if let Some(item) = item {
                            view! {
                                <ItemIcon icon_size=IconSize::Small item_id=listing.item_id/>
                                {&item.name}
                            }
                                .into_view()
                        } else {
                            view! { "Item not found" }
                                .into_view()
                        }}
                    </td>
                    <td>
                        <Gil amount=listing.price_per_unit/>
                    </td>
                    <td>{listing.quantity}</td>
                    <td>
                        <Gil amount=total/>
                    </td>
                </tr>
            }
        })
        .collect();
    view! {
        <div class="content-well">
            <span class="content-title">
                {retainer.name} " - " <WorldName id=AnySelector::World(retainer.world_id)/>
            </span>
            <table>
                <thead>
                    <tr>
                        <th>"HQ"</th>
                        <th>"Item"</th>
                        <th>"Price Per Unit"</th>
                        <th>"Quantity"</th>
                        <th>"Total"</th>
                    </tr>
                </thead>
                <tbody>{listings}</tbody>
            </table>
        </div>
    }
}

#[component]
pub(crate) fn CharacterRetainerList(
    character: Option<FfxivCharacter>,
    retainers: Vec<(Retainer, Vec<ActiveListing>)>,
) -> impl IntoView {
    let listings: Vec<_> = retainers
        .into_iter()
        .map(|(retainer, listings)| view! { <RetainerTable retainer listings/> })
        .collect();
    view! {
        <div>
            {if let Some(character) = character {
                view! { <span>{character.first_name} {character.last_name}</span> }
                    .into_view()
            } else {
                listings.into_view()
            }}
        </div>
    }
}

#[component]
pub fn RetainerUndercuts() -> impl IntoView {
    let retainers = create_resource(|| "undercuts", move |_| get_retainer_undercuts());
    view! {
        <MetaTitle title="Retainer Undercuts"/>
        <span class="content-title">"Retainer Undercuts"</span>
        <br/>
        <span>
            "Please keep in mind that data may not always be up to date. To update data, please contribute to universalis and then refresh this page."
        </span>
        <br/>
        <span>
            "This page will only show listings that have been undercut, enabling you to quickly view which items need to be refreshed"
        </span>
        <Suspense fallback=move || {
            view! { <Loading/> }
        }>
            {move || {
                retainers
                    .get()
                    .map(|retainer| {
                        match retainer {
                            Ok(retainers) => {
                                let retainers: Vec<_> = retainers
                                    .retainers
                                    .into_iter()
                                    .map(|(character, retainers)| {
                                        view! { <CharacterRetainerList character retainers/> }
                                    })
                                    .collect();
                                view! { <div>{retainers}</div> }
                                    .into_view()
                            }
                            Err(e) => {
                                view! { <div>{"Unable to get retainers"} <br/> {e.to_string()}</div> }
                                    .into_view()
                            }
                        }
                    })
            }}
        </Suspense>
    }
}

#[component]
pub fn RetainerListings() -> impl IntoView {
    let retainers = create_resource(|| "undercuts", move |_| get_retainer_listings());
    view! {
        <span class="content-title">"All Listings"</span>
        <MetaTitle title="All Listings"/>
        <MetaDescription text="View your retainer's listings without making it a second job!"/>
        <br/>
        <span>
            "Please keep in mind that data may not always be up to date. To update data, please contribute to universalis and then refresh this page."
        </span>
        <Suspense fallback=move || {
            view! { <Loading/> }
        }>
            {move || {
                retainers
                    .get()
                    .map(|retainer| {
                        match retainer {
                            Ok(retainers) => {
                                let retainers: Vec<_> = retainers
                                    .retainers
                                    .into_iter()
                                    .map(|(character, retainers)| {
                                        view! { <CharacterRetainerList character retainers/> }
                                    })
                                    .collect();
                                view! {
                                    {retainers
                                        .is_empty()
                                        .then(|| {
                                            view! { <span>"Add a retainer to get started!"</span> }
                                        })}
                                    <div>{retainers}</div>
                                }
                                    .into_view()
                            }
                            Err(e) => {
                                view! { <div>{"Unable to get retainers"} <br/> {e.to_string()}</div> }
                                    .into_view()
                            }
                        }
                    })
            }}
        </Suspense>
    }
}

#[component]
pub fn Retainers() -> impl IntoView {
    // let retainers = create_resource(|| "retainers", move |_| get_retainer_listings(cx));
    view! {
        <div class="content-nav">
            <A class="btn-secondary flex flex-row" href="/retainers/edit">
                <Icon width="1.75em" height="1.75em" icon=Icon::from(BsIcon::BsPencilFill)/>
                "Edit"
            </A>
            <A class="btn-secondary" href="/retainers/listings">
                "All Listings"
            </A>
            <A class="btn-secondary flex flex-row" href="/retainers/undercuts">
                <Icon width="1.75em" height="1.75em" icon=Icon::from(AiIcon::AiExclamationOutlined) />
                "Undercuts"
            </A>
        </div>
        <div
            class="main-content"
        >
        <div class="container mx-auto flex flex-col md:flex-row items-start">
            <div class="shrink">
                <Outlet />
            </div>
            <div class="md:grow">
                <Ad class="h-96 md:h-[50vh]"/>
            </div>
        </div>
        </div>
    }
}
