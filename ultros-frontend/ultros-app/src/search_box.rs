use crate::item::*;
use leptos::*;

#[component]
pub fn SearchBox(cx: Scope) -> impl IntoView {
    let (search, set_search) = create_signal(cx, String::new());
    let (active, set_active) = create_signal(cx, false);
    let on_input = move |ev| {
        set_search(event_target_value(&ev));
        set_active(true);
    };
    let focus_in = move |ev: web_sys::FocusEvent| set_active(true);
    leptos::log!("creating search");
    let items = &xiv_gen_db::decompress_data().items;
    leptos::log!("decompressed xiv_gen_db");
    // let items : Vec<(&xiv_gen::ItemId, &xiv_gen::Item)> = Vec::new();
    // let items = &items;
    let item_search = move || {
        search.with(|s| {
            items
                .into_iter()
                .filter(|(_, i)| i.name.contains(s))
                .collect::<Vec<_>>()
        })
    };
    view! {
        cx,
        <div>
            <input class="search-box" type="text" value=search on:input=on_input on:focus=focus_in/>
            <div class="search-results" class:active={move || active.get()}>
            // <For
            //     each=item_search
            //     key=|(id, _)| id.0
            //     view=move |(id, item): (&xiv_gen::ItemId, &xiv_gen::Item)| {
            //             let item_id = id.0;
            //             let item_name = item.name.clone();
            //             view! { cx,  <Item item_id item_name /> }
            //         }
            // />
            </div>
        </div>
    }
}
