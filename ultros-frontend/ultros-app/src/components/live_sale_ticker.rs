use super::gil::*;
use super::item_icon::*;
use super::loading::*;
use super::relative_time::*;
use std::collections::VecDeque;

use leptos::*;
use leptos_router::*;
use ultros_api_types::SaleHistory;
use ultros_api_types::UnknownCharacter;
use xiv_gen::ItemId;

use crate::global_state::home_world::get_homeworld;
#[cfg(not(feature = "ssr"))]
use crate::ws::live_data::live_sales;

#[component]
pub fn LiveSaleTicker(cx: Scope) -> impl IntoView {
    let sales = create_rw_signal::<VecDeque<(SaleHistory, UnknownCharacter)>>(cx, VecDeque::new());
    let (homeworld, _) = get_homeworld(cx);
    spawn_local(async move {
        #[cfg(not(feature = "ssr"))]
        if let Some(sale) = homeworld()
            .map(|homeworld| ultros_api_types::world_helper::AnySelector::World(homeworld.id))
        {
            log::info!("live sale");
            live_sales(sales, sale).await.unwrap();
        }
    });
    let items = &xiv_gen_db::decompress_data().items;
    view! {cx,
        <Suspense fallback=move || view!{cx, <Loading />}>
            {move ||{
                view!{cx,
                    <div class="content-well">
                    // <div class="content-title">{move || format!("Sales on {}", homeworld().map(|world| world.name).unwrap_or_default())}</div>
                    <div class="stock-ticker">
                        <div class="stock-ticker-body">
                            {move || sales()
                                    .into_iter()
                                    .flat_map(|(sale, character)| items.get(&ItemId(sale.sold_item_id))
                                    .map(|item| (item, sale, character)))
                                        .map(|(item, sale, character)| view!{cx,
                                            <div>
                                                <div class="flex-row">
                                                    <A class="flex-row" href=format!("/item/{}/{}", homeworld().map(|w| w.name).unwrap_or_default(), item.key_id.0)>
                                                    <ItemIcon item_id=item.key_id.0 icon_size=IconSize::Small />
                                                    <span>{&item.name}</span></A>
                                                    <Gil amount=sale.price_per_item />
                                                </div>
                                                <div class="flex-row" style="justify-content: space-between">
                                                    <span>{character.name}</span>
                                                    <span><RelativeToNow timestamp=sale.sold_date/></span>
                                                </div>
                                            </div>
                                        }).collect::<Vec<_>>()}
                        // <For each=sales
                        //     // the sale ID is just zero because I haven't figured out how to insert and fetch in an effiecient way...
                        //     // use the timestamp instead!
                        //     key=|(sale, _character)| sale.sold_date
                        //     view=|cx, (sale, character)| items.get(&ItemId(sale.sold_item_id)).map(|item| (item, sale, character))
                        //         .map(|(item, sale, character)| { view!{cx, <div>
                        //             <ItemIcon item_id=item.key_id.0 icon_size=IconSize::Small />
                        //             <span>{&item.name}</span>
                        //             <Gil amount=sale.price_per_item />
                        //             <span>{character.name}</span>
                        //             </div>} })

                        // />
                        </div>
                    </div>
                </div>}
            }}

    </Suspense>
    }
}
