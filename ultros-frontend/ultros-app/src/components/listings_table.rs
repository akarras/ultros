use super::gil::*;
use leptos::*;
use ultros_api_types::{ActiveListing, Retainer};

#[component]
pub fn ListingsTable(cx: Scope, listings: Vec<(ActiveListing, Retainer)>) -> impl IntoView {
    view! { cx,  <table>
            <tr>
                <th>"price"</th>
                <th>"qty."</th>
                <th>"total"</th>
                <th>"retainer name"</th>
                <th>"world"</th>
                <th>"datacenter"</th>
                <th>"first seen"</th>
            </tr>
        <For each=move || listings.clone()
        key=move |(listing, _retainer)| listing.id
        view=move |(listing, retainer)| {
            let total = listing.price_per_unit * listing.quantity;
            view! { cx, <tr>
                <td><Gil amount=listing.price_per_unit/></td>
                <td>{listing.quantity}</td>
                <td><Gil amount=total /></td>
                <td>{retainer.name}</td>
                <td>{listing.world_id}</td>
                <td>"datacenter"</td>
                <td>{listing.timestamp.to_string()}</td>
                </tr> }
        }
        />
    </table>
    }
}
