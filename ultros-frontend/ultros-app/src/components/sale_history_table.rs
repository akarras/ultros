use super::gil::*;
use leptos::*;
use ultros_api_types::SaleHistory;

#[component]
pub fn SaleHistoryTable(cx: Scope, sale_history: Vec<SaleHistory>) -> impl IntoView {
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
            // todo figure out why sale history isn't working
            <For each=move || sale_history.clone()
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
                            <td>{sale.world_id}</td>
                            <td>"datacenter"</td>
                            <td>{sale.sold_date.to_string()}</td>
                        </tr>
                    }
                }
            />
        </tbody>
    </table>
    }
}
