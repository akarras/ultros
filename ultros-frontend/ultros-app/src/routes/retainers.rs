use crate::api::{get_retainer_listings, get_retainers};
use crate::components::gil::*;
use crate::components::{item_icon::*, world_name::*};
use leptos::*;
use ultros_api_types::{world_helper::AnySelector, ActiveListing, FfxivCharacter, Retainer};
use xiv_gen::ItemId;

#[component]
fn RetainerView(cx: Scope) -> impl IntoView {
    view! {cx, <div></div>};
}

#[component]
fn RetainerTable(cx: Scope, retainer: Retainer, listings: Vec<ActiveListing>) -> impl IntoView {
    let items = &xiv_gen_db::decompress_data().items;
    let listings: Vec<_> = listings
        .into_iter()
        .map(|listing| {
            let item = items.get(&ItemId(listing.item_id));
            view! { cx, <tr><td>{if let Some(item) = item {
                view!{cx, <ItemIcon icon_size=IconSize::Medium item_id=listing.item_id />{&item.name}}.into_view(cx)
            } else {
                view!{cx, "Item not found"}.into_view(cx)
            }}</td><td><Gil amount=listing.price_per_unit/></td></tr>
        }})
        .collect();
    view! { cx,
        <div class="content-well">
            <span class="content-title">{retainer.name}" - "<WorldName id=AnySelector::World(retainer.world_id)/></span>
            <table>
                {listings}
            </table>
        </div>
    }
}

#[component]
fn CharacterRetainerList(
    cx: Scope,
    character: Option<FfxivCharacter>,
    retainers: Vec<(Retainer, Vec<ActiveListing>)>,
) -> impl IntoView {
    let listings: Vec<_> = retainers
        .into_iter()
        .map(|(retainer, listings)| view! {cx, <RetainerTable retainer listings />})
        .collect();
    view! {
        cx,
        <div class="content-well">
            {if let Some(character) = character {
                view!{cx, <span>{character.first_name} {character.last_name}</span> }.into_view(cx)
            } else {
                view!{cx, {listings}}.into_view(cx)
            }}
        </div>
    }
}

#[component]
pub fn Retainers(cx: Scope) -> impl IntoView {
    let retainers = create_resource(cx, || {}, move |()| get_retainer_listings(cx));
    view! {
        cx,
        <div class="container">
            <div class="content-nav">
                <a class="btn-secondary" href="/retainers/edit">
                    <span class="fa fa-pen-to-square"></span>
                    "Edit"
                </a>
                <a class="btn-secondary active" href="/retainers/edit">
                    <span class="fa fa-pencil"></span>
                    "Undercuts"
                </a>
                <a class="btn-secondary" href="/retainers/edit">
                    <span class="fa fa-exclamation"></span>
                    "Undercuts"
                </a>
            </div>
            <div class="main-content">
                <span class="content-title">"Retainers"</span>
                <Suspense fallback=move || view!{cx, <span>"Loading..."</span>}>
                {move || {
                    retainers.read().map(|retainer| {
                        match retainer {
                            Some(retainers) => {
                                let retainers : Vec<_> = retainers.retainers.into_iter()
                                    .map(|(character, retainers)| view!{cx, <CharacterRetainerList character retainers />})
                                    .collect();
                                view!{cx, <div>{retainers}</div>}
                            },
                            None => view!{cx, <div>"Unable to get retainers"</div>}
                        }
                    })
                }}
                </Suspense>
            </div>
        </div>
    }
}
