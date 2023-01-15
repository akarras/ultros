use crate::api::{get_retainer_listings, get_retainers};
use crate::components::gil::*;
use leptos::*;
use ultros_api_types::{ActiveListing, FfxivCharacter, Retainer};

#[component]
fn RetainerView(cx: Scope) -> impl IntoView {
    view! {cx, <div></div>};
}

#[component]
fn RetainerTable(cx: Scope, retainer: Retainer, listings: Vec<ActiveListing>) -> impl IntoView {
    let listings: Vec<_> = listings
        .into_iter()
        .map(|listing| view! { cx, <tr><Gil amount=listing.price_per_unit/></tr> })
        .collect();
    view! { cx,
        <div class="content-well">
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
            <div class="nav-secondary">
                <a class="btn btn-secondary" href="/retainers/edit"><span class="fa fa-pencil"></span>"Edit"</a>
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
