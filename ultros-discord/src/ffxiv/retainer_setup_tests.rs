//! Run the production command controller with deterministic external effects.
use super::*;
use std::{collections::BTreeMap, sync::Mutex};
use ultros_db::entity::active_listing;

#[derive(Default)]
struct State {
    characters: Vec<Character>,
    market: Vec<Retainer>,
    owned: BTreeMap<i32, Owned>,
    calls: Vec<String>,
    fail: Option<&'static str>,
}

#[derive(Default)]
struct FakeServices(Mutex<State>);

impl FakeServices {
    fn call(&self, operation: &str) -> Result<std::sync::MutexGuard<'_, State>, Error> {
        let mut state = self.0.lock().unwrap();
        state.calls.push(operation.into());
        if state.fail == Some(operation) {
            return Err(anyhow!("{operation} unavailable").into());
        }
        Ok(state)
    }
}

fn retainer(id: i32, world_id: i32) -> Retainer {
    Retainer {
        id,
        world_id,
        name: "Dawnkeeper".into(),
        retainer_city_id: 1,
    }
}

fn character(id: i32, world_id: i32) -> Character {
    Character {
        id,
        world_id,
        first_name: "Player".into(),
        last_name: format!("Alt{id}"),
    }
}

#[async_trait::async_trait]
impl RetainerServices for FakeServices {
    fn world_name(&self, id: i32) -> String {
        format!("World {id}")
    }
    fn world_id(&self, name: Option<&str>) -> Result<Option<i32>, Error> {
        name.filter(|s| !s.trim().is_empty())
            .map(|s| {
                s.trim()
                    .strip_prefix("World ")
                    .and_then(|s| s.parse().ok())
                    .ok_or_else(|| anyhow!("Unknown world").into())
            })
            .transpose()
    }
    fn locale(&self) -> Option<&str> {
        Some("en-US")
    }
    fn retainer_added(&self, owned: Owned) {
        self.0
            .lock()
            .unwrap()
            .calls
            .push(format!("added:{}", owned.retainer_id));
    }
    async fn characters(&self) -> Result<Vec<Character>, Error> {
        Ok(self.call("characters")?.characters.clone())
    }
    async fn search(&self, name: &str, world: Option<i32>) -> Result<Vec<Retainer>, Error> {
        Ok(self
            .call("search")?
            .market
            .iter()
            .filter(|r| {
                r.name.to_lowercase().starts_with(&name.to_lowercase())
                    && world.is_none_or(|world| r.world_id == world)
            })
            .cloned()
            .collect())
    }
    async fn retainer(&self, id: i32) -> Result<Option<Retainer>, Error> {
        Ok(self
            .call("retainer")?
            .market
            .iter()
            .find(|r| r.id == id)
            .cloned())
    }
    async fn save(&self, id: i32) -> Result<(Owned, bool), Error> {
        let mut state = self.call("save")?;
        if let Some(owned) = state.owned.get(&id) {
            return Ok((owned.clone(), false));
        }
        let owned = Owned {
            id,
            retainer_id: id,
            discord_id: 100,
            character_id: None,
            weight: Some(7),
        };
        state.owned.insert(id, owned.clone());
        Ok((owned, true))
    }
    async fn assign(&self, owned: i32, character: Option<i32>) -> Result<(), Error> {
        let mut state = self.call("assign")?;
        if character.is_some_and(|id| !state.characters.iter().any(|c| c.id == id)) {
            return Err(anyhow!("Not your character").into());
        }
        state
            .owned
            .get_mut(&owned)
            .ok_or_else(|| anyhow!("Not your retainer"))?
            .character_id = character;
        Ok(())
    }
    async fn retainers(&self) -> Result<Vec<(Owned, Retainer)>, Error> {
        let state = self.call("retainers")?;
        Ok(state
            .owned
            .values()
            .filter_map(|o| {
                state
                    .market
                    .iter()
                    .find(|r| r.id == o.retainer_id)
                    .map(|r| (o.clone(), r.clone()))
            })
            .collect())
    }
    async fn listings(&self, _: i32) -> Result<Vec<active_listing::Model>, Error> {
        drop(self.call("listings")?);
        Ok(vec![])
    }
    async fn remove(&self, owned: i32) -> Result<(), Error> {
        self.call("remove")?.owned.remove(&owned);
        Ok(())
    }
    async fn search_characters(
        &self,
        _: &str,
        _: Option<&str>,
    ) -> Result<Vec<lodestone::search::ProfileSearchResult>, Error> {
        drop(self.call("search_characters")?);
        Ok(vec![])
    }
    async fn claim_character(&self, id: u32) -> Result<Character, Error> {
        let mut state = self.call("claim_character")?;
        let c = character(id as i32, 10);
        state.characters.push(c.clone());
        Ok(c)
    }
}

/// Uses exactly the dispatcher and recovery/revision logic the gateway uses.
async fn click(
    session: &mut Session,
    services: &FakeServices,
    action: &str,
    selected: Option<i32>,
) -> Step {
    let id = format!("{}:{action}", session.revision);
    session.notice.clear();
    let result = session.dispatch(services, &id, selected).await;
    session.finish_step(result)
}

fn description(session: &Session) -> String {
    let reply = session.reply(|id| format!("World {id}"));
    assert_eq!(reply.ephemeral, Some(true));
    serde_json::to_value(&reply.embeds[0]).unwrap()["description"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn ambiguous_command_waits_for_choice_and_saves_the_chosen_world() {
    let services = FakeServices::default();
    services.0.lock().unwrap().market = vec![retainer(1, 10), retainer(2, 20)];
    let mut session = Session::start_add(&services, Some("Dawnkeeper".into()), None, None)
        .await
        .unwrap();
    assert!(services.0.lock().unwrap().owned.is_empty());
    assert_eq!(session.matches.len(), 2);
    click(&mut session, &services, "pick_retainer", Some(2)).await;
    assert_eq!(session.current.as_ref().unwrap().1.world_id, 20);
    assert!(description(&session).contains("Dawnkeeper"));
    assert_eq!(
        services.0.lock().unwrap().calls,
        ["characters", "search", "save", "added:2"]
    );
}

#[tokio::test]
async fn second_and_third_characters_work_on_same_and_different_worlds() {
    let services = FakeServices::default();
    services.0.lock().unwrap().characters =
        vec![character(1, 10), character(2, 10), character(3, 20)];
    services.0.lock().unwrap().market = vec![retainer(1, 10), retainer(2, 10), retainer(3, 20)];
    for id in 1..=3 {
        let mut session =
            Session::start_add(&services, Some(format!("retainer:{id}")), None, Some(id))
                .await
                .unwrap();
        assert_eq!(session.current.as_ref().unwrap().0.character_id, Some(id));
        assert!(description(&session).contains(&format!("Player Alt{id}")));
        click(&mut session, &services, "another", None).await;
        assert_eq!(session.adding_for.as_ref().unwrap().id, id);
        assert_eq!(
            session.search.world,
            Some(format!("World {}", if id == 3 { 20 } else { 10 }))
        );
    }
    assert_eq!(services.0.lock().unwrap().owned.len(), 3);
    let fresh = Session::start_add(&services, None, None, None)
        .await
        .unwrap();
    assert!(fresh.adding_for.is_none() && fresh.search.world.is_none());
}

#[tokio::test]
async fn duplicate_command_keeps_the_existing_character_and_order() {
    let services = FakeServices::default();
    services.0.lock().unwrap().characters = vec![character(1, 10), character(2, 10)];
    services.0.lock().unwrap().market = vec![retainer(1, 10)];
    Session::start_add(&services, Some("retainer:1".into()), None, Some(1))
        .await
        .unwrap();
    services.0.lock().unwrap().calls.clear();
    let session = Session::start_add(&services, Some("retainer:1".into()), None, Some(2))
        .await
        .unwrap();
    let owned = &session.current.as_ref().unwrap().0;
    assert_eq!((owned.character_id, owned.weight), (Some(1), Some(7)));
    assert_eq!(session.adding_for.as_ref().unwrap().id, 1);
    assert!(
        !services
            .0
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|c| c == "assign")
    );
    assert!(description(&session).contains("already tracking"));
}

#[tokio::test]
async fn failed_assignment_keeps_saved_retainer_and_can_be_retried() {
    let services = FakeServices::default();
    services.0.lock().unwrap().characters = vec![character(2, 10)];
    services.0.lock().unwrap().market = vec![retainer(1, 10)];
    services.0.lock().unwrap().fail = Some("assign");
    let mut session = Session::start_add(&services, Some("Dawnkeeper".into()), None, Some(2))
        .await
        .unwrap();
    assert_eq!(services.0.lock().unwrap().owned[&1].character_id, None);
    assert!(session.current.is_some());
    assert!(description(&session).contains("assign unavailable"));
    services.0.lock().unwrap().fail = None;
    click(&mut session, &services, "assign", None).await;
    click(&mut session, &services, "pick_character", Some(2)).await;
    assert_eq!(services.0.lock().unwrap().owned[&1].character_id, Some(2));
}

#[tokio::test]
async fn wrong_world_and_unoffered_character_ids_do_not_assign() {
    let services = FakeServices::default();
    services.0.lock().unwrap().characters = vec![character(2, 20)];
    services.0.lock().unwrap().market = vec![retainer(1, 10)];
    let mut session = Session::start_add(&services, Some("retainer:1".into()), None, Some(2))
        .await
        .unwrap();
    assert_eq!(session.view, View::Characters);
    assert!(description(&session).contains("still tracked"));
    click(&mut session, &services, "pick_character", Some(999)).await;
    assert!(description(&session).contains("Choose a character from this page"));
    assert!(
        !services
            .0
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|c| c == "assign")
    );
    click(&mut session, &services, "pick_character", Some(0)).await;
    assert_eq!(session.view, View::Saved);
    assert_eq!(session.current.as_ref().unwrap().0.character_id, None);
}

#[tokio::test]
async fn stale_remove_and_failed_remove_do_not_lose_the_retainer() {
    let services = FakeServices::default();
    services.0.lock().unwrap().market = vec![retainer(1, 10)];
    let mut session = Session::start_add(&services, Some("Dawnkeeper".into()), None, None)
        .await
        .unwrap();
    click(&mut session, &services, "remove", None).await;
    let result = session.dispatch(&services, "0:confirm_remove", None).await;
    session.finish_step(result);
    assert!(
        !services
            .0
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|c| c == "remove")
    );
    services.0.lock().unwrap().fail = Some("remove");
    click(&mut session, &services, "confirm_remove", None).await;
    assert!(session.current.is_some());
    assert!(description(&session).contains("completed changes have been kept"));
    services.0.lock().unwrap().fail = None;
    click(&mut session, &services, "confirm_remove", None).await;
    assert!(session.current.is_none() && services.0.lock().unwrap().owned.is_empty());
    assert!(description(&session).contains("Retainer removed"));
}

#[tokio::test]
async fn failed_list_is_a_retry_state_and_keeps_saved_records() {
    let services = FakeServices::default();
    services.0.lock().unwrap().market = vec![retainer(1, 10)];
    let mut session = Session::start_add(&services, Some("Dawnkeeper".into()), None, None)
        .await
        .unwrap();
    services.0.lock().unwrap().fail = Some("retainers");
    click(&mut session, &services, "list", None).await;
    assert!(!session.list_loaded);
    assert!(description(&session).contains("couldn't load"));
    services.0.lock().unwrap().fail = None;
    click(&mut session, &services, "list", None).await;
    assert!(session.list_loaded);
    assert!(description(&session).contains("Unassigned"));
    assert_eq!(session.retainers.len(), 1);
}

#[tokio::test]
async fn character_claim_failure_and_retry_use_only_the_visible_search_result() {
    let services = FakeServices::default();
    services.0.lock().unwrap().market = vec![retainer(1, 10)];
    let mut session = Session::start_add(&services, Some("Dawnkeeper".into()), None, None)
        .await
        .unwrap();
    session.view = View::CharacterResults;
    session.character_matches = vec![lodestone::search::ProfileSearchResult {
        user_id: 3,
        name: "Player Alt3".into(),
        world: "World 10".into(),
    }];
    click(&mut session, &services, "pick_new_character", Some(999)).await;
    assert!(
        !services
            .0
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|c| c == "claim_character")
    );
    services.0.lock().unwrap().fail = Some("claim_character");
    click(&mut session, &services, "pick_new_character", Some(3)).await;
    assert!(session.current.is_some());
    services.0.lock().unwrap().fail = None;
    click(&mut session, &services, "pick_new_character", Some(3)).await;
    assert_eq!(session.current.as_ref().unwrap().0.character_id, Some(3));
}

#[tokio::test]
async fn explicit_world_mismatch_does_not_save_an_autocomplete_choice() {
    let services = FakeServices::default();
    services.0.lock().unwrap().market = vec![retainer(1, 10)];
    let session = Session::start_add(
        &services,
        Some("retainer:1".into()),
        Some("World 20".into()),
        None,
    )
    .await
    .unwrap();
    assert!(session.current.is_none() && services.0.lock().unwrap().owned.is_empty());
    assert!(description(&session).contains("different world"));
}

#[tokio::test]
async fn failed_save_emits_no_event_and_the_same_search_can_be_retried() {
    let services = FakeServices::default();
    services.0.lock().unwrap().market = vec![retainer(1, 10)];
    services.0.lock().unwrap().fail = Some("save");
    let mut session = Session::start_add(&services, Some("Dawnkeeper".into()), None, None)
        .await
        .unwrap();
    assert!(session.current.is_none());
    assert!(services.0.lock().unwrap().owned.is_empty());
    assert!(
        !services
            .0
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|c| c.starts_with("added:"))
    );
    assert!(description(&session).contains("save unavailable"));
    services.0.lock().unwrap().fail = None;
    click(&mut session, &services, "all_worlds", None).await;
    assert_eq!(session.view, View::Saved);
    assert_eq!(
        services
            .0
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|c| c.starts_with("added:"))
            .count(),
        1
    );
}

#[tokio::test]
async fn lodestone_search_failure_keeps_the_search_defaults_and_saved_retainer() {
    let services = FakeServices::default();
    services.0.lock().unwrap().market = vec![retainer(1, 10)];
    let mut session = Session::start_add(&services, Some("Dawnkeeper".into()), None, None)
        .await
        .unwrap();
    let search = CharacterSearch {
        name: "Player Alt3".into(),
        world: Some("World 10".into()),
    };
    services.0.lock().unwrap().fail = Some("search_characters");
    let result = session
        .find_characters(&services, search.clone())
        .await
        .map(|()| Step::Updated);
    session.finish_step(result);
    assert!(session.current.is_some() && services.0.lock().unwrap().owned.contains_key(&1));
    assert_eq!(session.character_search.as_ref().unwrap().name, search.name);
    assert_eq!(
        session.character_search.as_ref().unwrap().world,
        search.world
    );
    assert!(description(&session).contains("search_characters unavailable"));
    services.0.lock().unwrap().fail = None;
    session.find_characters(&services, search).await.unwrap();
    assert_eq!(session.view, View::CharacterResults);
}
