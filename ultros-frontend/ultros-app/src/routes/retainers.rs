use crate::api::{get_retainer_listings, get_retainer_undercuts, get_user_retainer_listings};
use crate::components::ad::Ad;
use crate::components::icon::Icon;
use crate::components::retainer_table::{
    CharacterRetainerList, CharacterRetainerUndercutList, RetainerTable,
};
use crate::components::skeleton::BoxSkeleton;
use crate::components::{loading::*, meta::*};
use crate::global_state::LocalWorldData;
use components::{A, Outlet};
use hooks::use_params_map;
use icondata as i;
use leptos::either::Either;
use leptos::prelude::*;
use leptos_router::*;
use ultros_api_types::world_helper::AnySelector;

#[component]
pub fn RetainerUndercuts() -> impl IntoView {
    let retainers = Resource::new(|| "undercuts", move |_| get_retainer_undercuts());
    view! {
        <MetaTitle title="Retainer Undercuts" />
        <span class="content-title">"Retainer Undercuts"</span>
        <br />
        <span>
            "Please keep in mind that data may not always be up to date. To update data, please contribute to universalis and then refresh this page."
        </span>
        <br />
        <span>
            "This page will only show listings that have been undercut, enabling you to quickly view which items need to be refreshed"
        </span>
        <Suspense fallback=move || {
            view! { <Loading /> }
        }>
            {move || {
                retainers
                    .get()
                    .map(|retainer| {
                        match retainer {
                            Ok(retainers) => {
                                let retainers: Vec<_> = retainers
                                    .into_iter()
                                    .map(|(character, retainers)| {
                                        view! {
                                            <CharacterRetainerUndercutList character retainers />
                                        }
                                    })
                                    .collect();
                                Either::Left(view! { <div>{retainers}</div> })
                            }
                            Err(e) => {
                                Either::Right(
                                    view! {
                                        <div>
                                            {"Unable to get retainers"} <br /> {e.to_string()}
                                        </div>
                                    },
                                )
                            }
                        }
                    })
            }}

        </Suspense>
    }
}

#[component]
pub fn RetainersBasePath() -> impl IntoView {
    view! {
        <div>
            <h3>"Retainers"</h3>
            "Retainers can be added added to your account while logged in and tracked. To get started get logged in and click the tabs above."
        </div>
    }
}

#[component]
pub fn SingleRetainerListings() -> impl IntoView {
    let params = use_params_map();
    let retainer_listings = Resource::new(
        move || params().get("id").and_then(|id| id.parse::<i32>().ok()),
        move |id| async move {
            if let Some(id) = id {
                Some(get_retainer_listings(id).await)
            } else {
                None
            }
        },
    );

    view! {
        <span>
            "To claim this retainer, please login and visit "
            <A href="/retainers/edit">"the edit tab"</A>
        </span>
        <Suspense fallback=move || {
            view! {
                <div class="h-[300px] w-[600px]">
                    <BoxSkeleton />
                </div>
            }
        }>
            {move || {
                retainer_listings
                    .get()
                    .map(|r| {
                        r.and_then(|r| {
                            r.ok()
                                .map(|r| {
                                    let worlds = use_context::<LocalWorldData>()
                                        .expect("Local world data must be verified")
                                        .0
                                        .unwrap();
                                    let world = worlds
                                        .lookup_selector(AnySelector::World(r.retainer.world_id));
                                    let world_name = world
                                        .as_ref()
                                        .map(|w| w.get_name())
                                        .unwrap_or_default();
                                    view! {
                                        <MetaTitle title=format!(
                                            "{} - 🌍{}",
                                            &r.retainer.name,
                                            world_name,
                                        ) />
                                        <MetaDescription text=format!(
                                            "All of the listings for the retainer {} on the world {}",
                                            &r.retainer.name,
                                            world_name,
                                        ) />
                                        <RetainerTable retainer=r.retainer listings=r.listings />
                                    }
                                })
                        })
                    })
            }}

        </Suspense>
    }
}

#[component]
pub fn RetainerListings() -> impl IntoView {
    let retainers = Resource::new(|| "undercuts", move |_| get_user_retainer_listings());
    view! {
        <span class="content-title">"All Listings"</span>
        <MetaTitle title="All Listings" />
        <MetaDescription text="View your retainer's listings without making it a second job!" />
        <br />
        <span>
            "Please keep in mind that data may not always be up to date. To update data, please contribute to universalis and then refresh this page."
        </span>
        <Suspense fallback=move || {
            view! { <Loading /> }
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
                                        view! { <CharacterRetainerList character retainers /> }
                                    })
                                    .collect();
                                Either::Left(
                                    view! {
                                        {retainers
                                            .is_empty()
                                            .then(|| {
                                                view! { <span>"Add a retainer to get started!"</span> }
                                            })}

                                        <div>{retainers}</div>
                                    },
                                )
                            }
                            Err(e) => {
                                Either::Right(
                                    view! {
                                        <div>
                                            {"Unable to get retainers"} <br /> {e.to_string()}
                                        </div>
                                    },
                                )
                            }
                        }
                    })
            }}

        </Suspense>
    }.into_any()
}

#[component]
pub fn Retainers() -> impl IntoView {
    // let retainers = create_resource(|| "retainers", move |_| get_retainer_listings(cx));
    view! {
        <div class="flex items-center gap-2 md:gap-3 mb-3">
            <A exact=true attr:class="nav-link" href="/retainers/edit">
                <Icon height="1.25em" width="1.25em" icon=i::BsPencilFill />
                <span>"Edit"</span>
            </A>
            <A exact=true attr:class="nav-link" href="/retainers/listings">
                <Icon height="1.25em" width="1.25em" icon=i::AiOrderedListOutlined />
                <span>"All Listings"</span>
            </A>
            <A exact=true attr:class="nav-link" href="/retainers/undercuts">
                <Icon height="1.25em" width="1.25em" icon=i::AiExclamationOutlined />
                <span>"Undercuts"</span>
            </A>
        </div>
        <div class="main-content">
            <div class="container mx-auto flex flex-col xl:flex-row items-start">
                <div class="flex flex-col grow">
                    <div class="grow w-full">
                        <Ad class="h-[90px] w-full xl:w-[728px]" />
                    </div>
                    <Outlet />
                </div>
                <div>
                    <Ad class="h-96 w-96 xl:h-[750px] xl:w-40" />
                </div>
            </div>
        </div>
    }
    .into_any()
}
