use leptos::*;
use ultros_api_types::user::OwnedRetainer;
use ultros_api_types::world_helper::AnySelector;
use ultros_api_types::Retainer;

use crate::api::{
    claim_retainer, get_retainers, search_retainers, unclaim_retainer, update_retainer_order,
};
use crate::components::{loading::*, reorderable_list::*, retainer_nav::*, world_name::*};

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

    let claim = create_action(cx, move |retainer_id| claim_retainer(cx, *retainer_id));

    let remove_retainer = create_action(cx, move |owned_id| unclaim_retainer(cx, *owned_id));
    let update_retainers = create_action(cx, move |owners: &Vec<OwnedRetainer>| {
        update_retainer_order(cx, owners.clone())
    });
    let retainers = create_resource(
        cx,
        move || {
            (
                claim.version().get(),
                remove_retainer.version().get(),
                // update_retainers.version().get(),
            )
        },
        move |key| {
            log::info!("getting retainers {key:?}");
            get_retainers(cx)
        },
    );

    let is_retainer_owned = move |retainer_id: i32| {
        retainers
            .with(cx, |retainer| {
                retainer
                    .as_ref()
                    .map(|retainers| {
                        retainers.retainers.iter().any(|(_character, retainers)| {
                            retainers
                                .iter()
                                .any(|(_, retainer)| retainer.id == retainer_id)
                        })
                    })
                    .ok()
            })
            .flatten()
            .unwrap_or_default()
    };

    view! { cx,
    <div class="container">
      <RetainerNav/>
      <div class="main-content flex-wrap">
        <div style="width: 500px;" class="retainer-list flex-column">
          <span class="content-title">"Retainers"</span>
          <Transition fallback=move || view!{cx, <div></div>}>
            {move || retainers.read(cx).map(|retainers| {
              match retainers {
                Ok(retainers) => {

                  view!{cx,
                      {move || update_retainers.value()().map(|value| {
                        match value {
                          Ok(_) => None,
                          Err(e) => Some(format!("App error: {e:?}"))
                        }
                      })}
                      <For each=move || retainers.retainers.clone()
                        key=move |(character, retainers)| (character.as_ref().map(|c| c.id).unwrap_or_default(), retainers.iter().map(|(o, _r)| o.id).collect::<Vec<_>>())
                        view=move |cx, (character, retainers)| {
                          let retainers = create_rw_signal(cx, retainers);
                          create_effect(cx, move |_| {
                            let retainers = retainers();
                            let mut changed = false;
                            let retainers = retainers.into_iter().enumerate().flat_map(|(i, (mut owned, _retainer))| {
                              if let Some(weight) = &mut owned.weight {
                                if *weight != i as i32 {
                                  changed = true;
                                  *weight = i as i32;
                                  return Some(owned)
                                }
                              } else {
                                owned.weight = Some(i as i32);
                                changed = true;
                                return Some(owned)
                              }
                              None
                            }).collect();
                            // I have no idea how I would have found that the #[server] macro takes params as a struct
                            // without the compiler just spelling it out for me
                            if changed {
                              log::info!("Updating retainer list");
                              update_retainers.dispatch(retainers);
                            }
                          });
                          view!{cx,
                          {if let Some(character) = character {
                            view!{cx, <div>{character.first_name}" "{character.last_name}</div>}
                          } else {
                            view!{cx, <div>"No character"</div>}
                          }}
                        <div class="flex-column">
                          <ReorderableList items=retainers item_view=move |cx, (owned, retainer): (OwnedRetainer, Retainer)| {
                            let owned_id = owned.id;
                            let retainer_name = retainer.name.to_string();
                            let world_id = retainer.world_id;
                            view!{
                            cx,
                            <div class="flex-row">
                              <div style="width: 300px" class="flex">
                                <span style="width: 200px">{retainer_name}</span>
                                <span><WorldName id=AnySelector::World(world_id)/></span>
                                </div>
                              <button class="btn" on:click=move |_| remove_retainer.dispatch(owned_id)>"Unclaim"</button>
                              </div>
                          }} />
                        </div>} }
                      />}.into_view(cx)
                },
                Err(e) => {
                  view!{cx, <div>"Retainers"<br/>{e.to_string()}</div>}.into_view(cx)
                }
              }
            })}
          </Transition>
        </div>
        <div class="retainer-search">
            <span class="content-title">"Search:"</span>
          <input prop:value=retainer_search  on:input=move |input| set_retainer_search(event_target_value(&input)) />
          <div class="retainer-results">
            <Suspense fallback=move || view!{cx, <Loading/>}>
              {move || search_results.read(cx).map(|retainers| {
                match retainers {
                  Ok(retainers) => view!{cx, <div class="content-well flex-column">
                    <For each=move || retainers.clone()
                          key=move |retainer| retainer.id
                          view=move |cx, retainer| {
                            let world = AnySelector::World(retainer.world_id);
                            view!{ cx, <div class="card flex-row">
                              <div style="width: 300px" class="flex">
                                <span style="width: 200px;">{retainer.name}</span>
                                <WorldName id=world/>
                              </div>
                              <button class="btn" on:click=move |_| claim.dispatch(retainer.id)>{move || match is_retainer_owned(retainer.id) {
                                true => "Claimed",
                                false => "Claim"
                              }}</button>
                            </div>}
                          }
                          />
                  </div>}.into_view(cx),
                  Err(e) => view!{cx, <div>{format!("No retainers found\n{e}")}</div>}.into_view(cx)
                }
              })}
            </Suspense>
          </div>
        </div>
      </div>
    </div>}
}
