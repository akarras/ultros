use super::gil::*;
use super::relative_time::*;
use crate::components::{datacenter_name::*, world_name::*};
use leptos::*;
use ultros_api_types::{world_helper::AnySelector, ActiveListing, Retainer};

#[component]
pub fn ListingsTable(listings: Vec<(ActiveListing, Retainer)>) -> impl IntoView {
    let (show_more, set_show_more) = create_signal(false);
    let listing_count = listings.len();
    let show_click = move |_| set_show_more(true);
    let listings = create_memo(move |_| {
        let mut listings = listings.clone();
        listings.sort_by_key(|(listing, _)| listing.price_per_unit);
        if show_more() {
            listings.clone()
        } else {
            listings.iter().take(10).cloned().collect()
        }
    });
    view! {  <table>
            <tr>
                <th>"price"</th>
                <th>"qty."</th>
                <th>"total"</th>
                <th>"retainer name"</th>
                <th>"world"</th>
                <th>"datacenter"</th>
                <th>"first seen"</th>
            </tr>
        <tbody>
        {move || view!{<For each=listings
        key=move |(listing, _retainer)| listing.id
        children=move |(listing, retainer)| {
            let total = listing.price_per_unit * listing.quantity;
            view! { <tr>
                <td><Gil amount=listing.price_per_unit/></td>
                <td>{listing.quantity}</td>
                <td><Gil amount=total /></td>
                <td>{retainer.name}</td>
                <td><WorldName id=AnySelector::World(listing.world_id) /></td>
                <td><DatacenterName world_id=listing.world_id/> </td>
                <td><RelativeToNow timestamp=listing.timestamp/></td>
                </tr> }
        }
        />}}
        <tr on:click=show_click class:hidden=move || {listing_count < 10 || show_more() }>
            <td colspan=7>
                <button on:click=show_click style="width: 100%;" class="btn">"Show More"</button>
            </td>
        </tr>
        </tbody>
    </table>
    }
}
