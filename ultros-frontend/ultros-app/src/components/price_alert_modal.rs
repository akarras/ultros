use crate::ws::alerts::AlertsService;
use leptos::prelude::*;
use ultros_api_types::world_helper::AnySelector;

#[component]
pub fn PriceAlertModal(show: RwSignal<bool>, item_id: i32, world_name: String) -> impl IntoView {
    let alerts_service = use_context::<AlertsService>().unwrap();
    let (price, set_price) = signal(0);
    let (scope, set_scope) = signal("World".to_string()); // World, Data Center, Region

    let world_name_stored = StoredValue::new(world_name.clone());
    let on_submit = move |_| {
        world_name_stored.with_value(|world_name| {
            // Resolve scope to AnySelector
            let world_data = use_context::<crate::global_state::LocalWorldData>()
                .unwrap()
                .0
                .unwrap();
            if let Some(any_result) = world_data.lookup_world_by_name(world_name) {
                let selector = match scope.get_untracked().as_str() {
                    "World" => AnySelector::World(any_result.as_world().unwrap().id),
                    "Data Center" => {
                        let dc = world_data.get_datacenters(&any_result)[0];
                        AnySelector::Datacenter(dc.id)
                    }
                    "Region" => {
                        let region = world_data.get_region(any_result);
                        AnySelector::Region(region.id)
                    }
                    _ => return,
                };

                alerts_service.create_price_alert(item_id, price.get_untracked(), selector);
                show.set(false);
                AlertsService::request_permission();
            }
        });
    };

    view! {
        <Show when=move || show.get()>
            <div class="fixed inset-0 z-[100] flex items-center justify-center bg-black/50 backdrop-blur-sm">
                <div class="panel p-6 w-full max-w-md space-y-4">
                    <h2 class="text-xl font-bold text-white">"Create Price Alert"</h2>

                    <div class="space-y-2">
                        <label class="block text-sm font-medium text-gray-300">"Price Threshold (Gil)"</label>
                        <input
                            type="number"
                            class="input w-full"
                            prop:value=move || price.get()
                            on:input=move |ev| set_price.set(event_target_value(&ev).parse().unwrap_or(0))
                        />
                    </div>

                    <div class="space-y-2">
                        <label class="block text-sm font-medium text-gray-300">"Scope"</label>
                        <select
                            class="input w-full"
                            on:change=move |ev| set_scope.set(event_target_value(&ev))
                        >
                            <option value="World">"Current World (" {world_name.clone()} ")"</option>
                            <option value="Data Center">"Data Center"</option>
                            <option value="Region">"Region"</option>
                        </select>
                    </div>

                    <div class="flex justify-end gap-2 pt-4">
                        <button class="btn" on:click=move |_| show.set(false)>"Cancel"</button>
                        <button class="btn-primary" on:click=on_submit>"Create Alert"</button>
                    </div>
                </div>
            </div>
        </Show>
    }
}
