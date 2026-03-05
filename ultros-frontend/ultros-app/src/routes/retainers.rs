use crate::api::{
    UndercutData, get_retainer_listings, get_retainer_undercuts, get_user_retainer_listings,
};
use crate::components::ad::Ad;
use crate::components::clipboard::Clipboard;
use crate::components::gil::*;
use crate::components::icon::Icon;
use crate::components::skeleton::BoxSkeleton;
use crate::components::{item_icon::*, loading::*, meta::*, world_name::*};
use crate::global_state::LocalWorldData;
use crate::i18n::*;
use components::{A, Outlet};
use hooks::use_params_map;
use icondata as i;
use leptos::either::Either;
use leptos::prelude::*;
use leptos_router::*;
use ultros_api_types::{ActiveListing, FfxivCharacter, Retainer, world_helper::AnySelector};
use xiv_gen::{ItemId, ItemSortCategoryId};

#[derive(PartialOrd, Ord, Eq, PartialEq, Debug)]
struct ItemSortKey(u8, i32, bool);

impl From<(ItemId, bool)> for ItemSortKey {
    fn from((item_id, hq): (ItemId, bool)) -> Self {
        let inner = move || {
            let data = xiv_gen_db::data();
            let items = &data.items;
            let sort_category = &data.item_sort_categorys;
            let item = items.get(&item_id)?;
            let sort_weight = sort_category
                .get(&ItemSortCategoryId(item.item_sort_category as i32))
                .map(|category| category.param)?;
            Some(Self(sort_weight as u8, item.key_id.0, hq))
        };
        inner().unwrap_or(Self(u8::MAX, i32::MAX, hq))
    }
}

impl From<&ActiveListing> for ItemSortKey {
    fn from(listing: &ActiveListing) -> Self {
        ItemSortKey::from((ItemId(listing.item_id), listing.hq))
    }
}

#[component]
fn RetainerUndercutTable(retainer: Retainer, listings: Vec<UndercutData>) -> impl IntoView {
    let i18n = use_i18n();
    let mut listings = listings;
    let data = xiv_gen_db::data();
    let items = &data.items;
    listings.sort_by_key(|u| ItemSortKey::from(&u.current));
    let worlds = use_context::<LocalWorldData>().unwrap().0.unwrap();
    let world = worlds.lookup_selector(AnySelector::World(retainer.world_id));
    let world_name = world.as_ref().map(|w| w.get_name()).unwrap_or_default();
    let listings: Vec<_> = listings
        .into_iter()
        .map(|undercut_data| {
            let listing = undercut_data.current;
            let item = items.get(&ItemId(listing.item_id));
            let total = listing.quantity * listing.price_per_unit;
            let new_best_price = undercut_data.cheapest - 1;
            view! {
                <tr>
                    <td>{listing.hq.then(|| t!(i18n, retainers_hq))}</td>
                    <td class="flex flex-row">
                        {if let Some(item) = item {
                            Either::Left(
                                view! {
                                    <A
                                        attr:class="flex flex-row"
                                        href=format!("/item/{world_name}/{}", listing.item_id)
                                    >
                                        <ItemIcon
                                            icon_size=IconSize::Small
                                            item_id=listing.item_id
                                        />
                                        {item.name.as_str()}
                                    </A>
                                    <Clipboard clipboard_text=item.name.as_str() />
                                },
                            )
                        } else {
                            Either::Right(view! { {t!(i18n, retainers_item_not_found)} })
                        }}

                    </td>
                    <td>
                        <Gil amount=listing.price_per_unit />
                    </td>
                    <td>{listing.quantity}</td>
                    <td>
                        <Gil amount=total />
                    </td>
                    <td>
                        <div class="flex flex-row">
                            <Gil amount=new_best_price />
                            <Clipboard clipboard_text=new_best_price.to_string() />
                        </div>
                    </td>
                </tr>
            }
            .into_any()
        })
        .collect();
    view! {
        <div class="panel p-4 rounded-xl">
            <span class="content-title">
                {retainer.name} " - " <WorldName id=AnySelector::World(retainer.world_id) />
            </span>
            <table class="w-full">
                <thead>
                    <tr>
                        <th>{t!(i18n, retainers_hq)}</th>
                        <th>{t!(i18n, retainers_item)}</th>
                        <th>{t!(i18n, retainers_price_per_unit)}</th>
                        <th>{t!(i18n, retainers_quantity)}</th>
                        <th>{t!(i18n, retainers_total)}</th>
                        <th>{t!(i18n, retainers_undercut_by_one)}</th>
                    </tr>
                </thead>
                <tbody>{listings}</tbody>
            </table>
        </div>
    }
    .into_any()
}

#[component]
fn RetainerTable(retainer: Retainer, listings: Vec<ActiveListing>) -> impl IntoView {
    let i18n = use_i18n();
    let data = xiv_gen_db::data();
    let items = &data.items;
    let mut listings = listings;
    listings.sort_by_key(|u| ItemSortKey::from(u));
    let world_data = use_context::<LocalWorldData>().unwrap();
    let worlds = world_data.0.unwrap();
    let world = worlds.lookup_selector(AnySelector::World(retainer.world_id));
    let world_name = world.as_ref().map(|w| w.get_name()).unwrap_or_default();
    let listings: Vec<_> = listings
        .into_iter()
        .map(|listing| {
            let item = items.get(&ItemId(listing.item_id));
            let total = listing.quantity * listing.price_per_unit;
            view! {
                <tr>
                    <td>{listing.hq.then(|| t!(i18n, retainers_hq))}</td>
                    <td class="flex flex-row">
                        {if let Some(item) = item {
                            Either::Left(
                                view! {
                                    <A
                                        attr:class="flex flex-row"
                                        href=format!("/item/{}/{}", world_name, listing.item_id)
                                    >
                                        <ItemIcon
                                            icon_size=IconSize::Small
                                            item_id=listing.item_id
                                        />
                                        {item.name.as_str()}
                                    </A>
                                    <Clipboard clipboard_text=item.name.as_str() />
                                },
                            )
                        } else {
                            Either::Right(view! { {t!(i18n, retainers_item_not_found)} })
                        }}

                    </td>
                    <td>
                        <Gil amount=listing.price_per_unit />
                    </td>
                    <td>{listing.quantity}</td>
                    <td>
                        <Gil amount=total />
                    </td>
                </tr>
            }
            .into_any()
        })
        .collect();
    view! {
        <div class="panel p-4 rounded-xl">
            <span class="content-title">
                {retainer.name} " - " <WorldName id=AnySelector::World(retainer.world_id) />
            </span>
            <table>
                <thead>
                    <tr>
                        <th>{t!(i18n, retainers_hq)}</th>
                        <th>{t!(i18n, retainers_item)}</th>
                        <th>{t!(i18n, retainers_price_per_unit)}</th>
                        <th>{t!(i18n, retainers_quantity)}</th>
                        <th>{t!(i18n, retainers_total)}</th>
                    </tr>
                </thead>
                <tbody>{listings}</tbody>
            </table>
        </div>
    }
    .into_any()
}

#[component]
pub(crate) fn CharacterRetainerList(
    character: Option<FfxivCharacter>,
    retainers: Vec<(Retainer, Vec<ActiveListing>)>,
) -> impl IntoView {
    let listings: Vec<_> = retainers
        .into_iter()
        .map(|(retainer, listings)| view! { <RetainerTable retainer listings /> })
        .collect();
    view! {
        <div>
            {if let Some(character) = character {
                Either::Left(view! { <span>{character.first_name} {character.last_name}</span> })
            } else {
                Either::Right(listings)
            }}

        </div>
    }
    .into_any()
}

#[component]
pub(crate) fn CharacterRetainerUndercutList(
    character: Option<FfxivCharacter>,
    retainers: Vec<(Retainer, Vec<UndercutData>)>,
) -> impl IntoView {
    let listings: Vec<_> = retainers
        .into_iter()
        .map(|(retainer, listings)| view! { <RetainerUndercutTable retainer listings /> })
        .collect();
    view! {
        <div>
            {if let Some(character) = character {
                Either::Left(
                    view! { <span>{character.first_name} {character.last_name}</span> }.into_view(),
                )
            } else {
                Either::Right(listings)
            }}

        </div>
    }
    .into_any()
}

#[component]
pub fn RetainerUndercuts() -> impl IntoView {
    let i18n = use_i18n();
    let retainers = Resource::new(|| "undercuts", move |_| get_retainer_undercuts());
    view! {
        <MetaTitle title=t_string!(i18n, retainers_undercuts_title).to_string() />
        <span class="content-title">{t!(i18n, retainers_undercuts_title)}</span>
        <br />
        <span>
            {t!(i18n, retainers_data_notice)}
        </span>
        <br />
        <span>
            {t!(i18n, retainers_undercuts_description)}
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
                                                    {t!(i18n, retainers_unable_to_get)} <br /> {e.to_string()}
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
    let i18n = use_i18n();
    view! {
        <div>
            <h3>{t!(i18n, retainers_title)}</h3>
            {t!(i18n, retainers_base_path_description)}
        </div>
    }
}

#[component]
pub fn SingleRetainerListings() -> impl IntoView {
    let i18n = use_i18n();
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
            {t!(i18n, retainers_claim_prompt_start)}
            <A href="/retainers/edit">{t!(i18n, retainers_claim_prompt_link)}</A>
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
    let i18n = use_i18n();
    let retainers = Resource::new(|| "undercuts", move |_| get_user_retainer_listings());
    view! {
        <span class="content-title">{t!(i18n, retainers_all_listings_title)}</span>
        <MetaTitle title=t_string!(i18n, retainers_all_listings_title).to_string() />
        <MetaDescription text=t_string!(i18n, retainers_all_listings_desc).to_string() />
        <br />
        <span>
            {t!(i18n, retainers_data_notice)}
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
                                                view! { <span>{t!(i18n, retainers_add_to_start)}</span> }
                                            })}

                                        <div>{retainers}</div>
                                    },
                                )
                            }
                                    Err(e) => {
                                        Either::Right(
                                            view! {
                                                <div>
                                                    {t!(i18n, retainers_unable_to_get)} <br /> {e.to_string()}
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
    let i18n = use_i18n();
    // let retainers = create_resource(|| "retainers", move |_| get_retainer_listings(cx));
    view! {
        <div class="flex items-center gap-2 md:gap-3 mb-3">
            <A exact=true attr:class="nav-link" href="/retainers/edit">
                <Icon height="1.25em" width="1.25em" icon=i::BsPencilFill />
                <span>{t!(i18n, retainers_edit_tab)}</span>
            </A>
            <A exact=true attr:class="nav-link" href="/retainers/listings">
                <Icon height="1.25em" width="1.25em" icon=i::AiOrderedListOutlined />
                <span>{t!(i18n, retainers_all_listings_tab)}</span>
            </A>
            <A exact=true attr:class="nav-link" href="/retainers/undercuts">
                <Icon height="1.25em" width="1.25em" icon=i::AiExclamationOutlined />
                <span>{t!(i18n, retainers_undercuts_tab)}</span>
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

#[cfg(test)]
mod test {

    use super::ItemSortKey;

    #[cfg(feature = "ssr")]
    #[test]
    fn test_sort_order() {
        // these item ids are in the correct order- so if we run it through our sort, it should still match up
        use chrono::NaiveDateTime;
        use ultros_api_types::ActiveListing;
        let item_ids = vec![
            29417, 30842, 36837, 31840, 17325, 9050, 15532, 4737, 19853, 24250,
        ];
        let mut item_vec: Vec<_> = item_ids
            .into_iter()
            .map(|item| ActiveListing {
                id: 0,
                world_id: 0,
                item_id: item,
                retainer_id: 0,
                price_per_unit: 1000,
                quantity: 1,
                hq: true,
                timestamp: NaiveDateTime::MIN,
            })
            .collect();
        let original = item_vec.clone();
        item_vec.sort_by_key(|i| ItemSortKey::from(i));
        assert_eq!(original, item_vec);
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn same_sort_category() {
        use xiv_gen::ItemId;

        let expected_order = vec![
            41509, // red corsage
            41516, // black corsage
            41517,
        ]; // rainbow corsage
        let mut rearranged = vec![41516, 41517, 41509];
        rearranged.sort_by_key(|id| ItemSortKey::from((ItemId(*id), true)));
        assert_eq!(expected_order, rearranged);
    }
}
