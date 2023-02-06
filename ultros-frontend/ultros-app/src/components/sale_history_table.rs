use super::{datacenter_name::*, gil::*, relative_time::*, world_name::*};
use leptos::*;
use ultros_api_types::{world_helper::AnySelector, SaleHistory};

#[component]
pub fn SaleHistoryTable(cx: Scope, sales: MaybeSignal<Vec<SaleHistory>>) -> impl IntoView {
    let (show_more, set_show_more) = create_signal(cx, false);
    let sale_history = create_memo(cx, move |_| {
        let mut sales = sales();
        if !show_more() {
            sales.truncate(10);
        }
        sales
    });
    view! { cx,  <table>
        <thead>
            <tr>
                <th>"hq"</th>
                <th>"price"</th>
                <th>"qty."</th>
                <th>"total"</th>
                <th>"retainer name"</th>
                <th>"world"</th>
                <th>"datacenter"</th>
                <th>"first seen"</th>
            </tr>
        </thead>
        <tbody>
            <For each=sale_history
                key=move |sale| sale.sold_date.timestamp()
                view=move |sale| {
                    let total = sale.price_per_item * sale.quantity;
                    view! { cx,
                        <tr>
                            <td>{if sale.hq {view!{cx, <span class="fa-solid fa-check"></span>}.into_view(cx)} else {
                                view!{cx, }.into_view(cx)
                            }}</td>
                            <td><Gil amount=sale.price_per_item/></td>
                            <td>{sale.quantity}</td>
                            <td><Gil amount=total /></td>
                            <td>{sale.buyer_name}</td>
                            <td><WorldName id=AnySelector::World(sale.world_id)/></td>
                            <td><DatacenterName world_id=sale.world_id/></td>
                            <td><RelativeToNow timestamp=sale.sold_date/></td>
                        </tr>
                    }
                }
            />
            {move || (!show_more() && sale_history.with(|sales| sales.len() > 10)).then(|| {
                view!{cx, <button class="btn" on:click=move |_| set_show_more(true)>"Show more"</button>}
            })}
        </tbody>
    </table>
    }
}
