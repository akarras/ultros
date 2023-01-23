use leptos::*;
use ultros_api_types::world_helper::AnySelector;
use leptos_router::*;

use crate::api::{search_retainers, claim_retainer};
use crate::components::world_name::*;

#[component]
pub fn EditRetainers(cx: Scope) -> impl IntoView {
    // This page should let the user drag and drop retainers to reorder them
    // It should also support a search panel for retainers to the right that will allow the user to search for retainers

    let (retainer_search, set_retainer_search) = create_signal(cx, String::new());

    let search_results = create_resource(
        cx,
        move || retainer_search(),
        move |search| async move { search_retainers(cx, search).await },
    );

    // let claim = create_server_action(cx, |retainer_id: &i32| {
    //   claim_retainer(cx, *retainer_id)
    // });
    view! { cx,
    <div class="container">
      <div class="main-content">
        <div class="retainer-search">
          <input prop:value=retainer_search  on:input=move |input| set_retainer_search(event_target_value(&input)) />
          <div class="retainer-results">
            <Suspense fallback=move || view!{cx, <div class="loading"></div>}>
              {move || search_results.read().map(|retainers| {
                match retainers {
                  Some(retainers) => view!{cx, <div class="content-well flex-wrap">
                    <For each=move || retainers.clone()
                          key=move |retainer| retainer.id
                          view=move |retainer| {
                            let world = AnySelector::World(retainer.world_id);
                            view!{ cx, <div class="card flex-column">
                              <span>{retainer.name}</span>
                              <WorldName id=world/>
                              // <ActionForm action=claim>
                              //   <input type="submit" value="claim"/>
                              // </ActionForm>
                            </div>}
                          }
                          />
                  </div>}.into_view(cx),
                  None => view!{cx, <div>"No retainers found"</div>}.into_view(cx)
                }
              })}
            </Suspense>
          </div>
        </div>
      </div>
    </div>}
}
