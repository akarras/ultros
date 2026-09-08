//! `ListSync`: the merge path plus everything the rest of the product needs
//! to hear about it (spec sections 4.2, 4.4, 4.5, 5).

use std::sync::Arc;

use tracing::warn;
use ultros_api_types::list::{List, ListActivity};
use ultros_api_types::websocket::{ListDocPayload, ListEventData};
use ultros_db::UltrosDb;
use ultros_db::list_doc::{ListDocError, MergeOutcome, ProjectedChange};
use ultros_list_doc::{DocError, ListDocument, RowChange, SyncPayload};

use crate::alerts::price_alert_tracker::resolve_item_name;
use crate::event::{EventSenders, EventType, ListDocEvent};
use crate::lists::activity::entries_for;
use crate::web::oauth::AuthDiscordUser;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    Socket(u64),
    Rest,
    Bot,
}

#[derive(Clone, Debug)]
pub(crate) struct Actor {
    pub user_id: i64,
    pub username: String,
    pub origin: Origin,
}

impl Actor {
    pub(crate) fn from_user(user: &AuthDiscordUser, origin: Origin) -> Self {
        Self {
            user_id: user.id as i64,
            username: user.name.clone(),
            origin,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Applied {
    pub relay: Vec<u8>,
    pub changes: Vec<ProjectedChange>,
    pub meta_changed: bool,
}

#[derive(Clone)]
pub(crate) struct ListSync {
    db: UltrosDb,
    senders: EventSenders,
}

impl ListSync {
    pub(crate) fn new(db: UltrosDb, senders: EventSenders) -> Self {
        Self { db, senders }
    }

    /// Bytes from a peer.
    pub(crate) async fn apply_update(
        &self,
        list_id: i32,
        actor: &Actor,
        update: &[u8],
    ) -> Result<Applied, ListDocError> {
        let outcome = self
            .db
            .apply_list_update(list_id, actor.user_id, update)
            .await?;
        Ok(self.publish(list_id, actor, outcome).await)
    }

    /// A legacy writer's edit, made by the server peer on the actor's behalf.
    pub(crate) async fn edit_as_server<R>(
        &self,
        list_id: i32,
        actor: &Actor,
        edit: impl FnOnce(&ListDocument) -> Result<R, DocError>,
    ) -> Result<(R, Applied), ListDocError> {
        let (value, outcome) = self.db.edit_list_doc(list_id, actor.user_id, edit).await?;
        Ok((value, self.publish(list_id, actor, outcome).await))
    }

    /// The subscribe handshake: the server's version and what the client lacks.
    pub(crate) async fn subscribe_payload(
        &self,
        list_id: i32,
        user_id: i64,
        client_version: &[u8],
    ) -> Result<(Vec<u8>, ListDocPayload), ListDocError> {
        let stored = self.db.list_doc_snapshot(list_id, user_id).await?;
        let doc = ListDocument::from_snapshot(&stored.snapshot)?;
        let payload = match doc.sync_payload(client_version)? {
            SyncPayload::Snapshot(bytes) => ListDocPayload::Snapshot(bytes),
            SyncPayload::Updates(bytes) => ListDocPayload::Updates(bytes),
            SyncPayload::UpToDate => ListDocPayload::UpToDate,
        };
        Ok((doc.version(), payload))
    }

    /// Legacy events for the old page and the alert trackers, activity rows,
    /// and the relay to other document subscribers. Best effort: a failure
    /// here is logged, never returned, because the merge already committed.
    async fn publish(&self, list_id: i32, actor: &Actor, outcome: MergeOutcome) -> Applied {
        for change in &outcome.changes {
            let item = ultros_api_types::list::ListItem::from(change.row.clone());
            let event = match change.change {
                RowChange::Added(_) => EventType::added(ListEventData::ListItem(item)),
                RowChange::Removed(_) => EventType::removed(ListEventData::ListItem(item)),
                RowChange::Updated { .. } => EventType::updated(ListEventData::ListItem(item)),
            };
            if let Err(e) = self.senders.lists.send(event) {
                warn!(error = %e, "failed to broadcast list item event");
            }
        }
        if outcome.meta_changed() {
            match List::try_from(outcome.list.clone()) {
                Ok(list) => {
                    if let Err(e) = self
                        .senders
                        .lists
                        .send(EventType::updated(ListEventData::List(list)))
                    {
                        warn!(error = %e, "failed to broadcast list event");
                    }
                }
                Err(e) => warn!(error = %e, list_id, "list row has no scope"),
            }
        }

        let meta = outcome.meta_changed().then_some(&outcome.meta_after);
        let entries = entries_for(&actor.username, &outcome.changes, meta, resolve_item_name);
        if !entries.is_empty()
            && let Err(e) = self
                .db
                .get_or_create_discord_user(actor.user_id as u64, actor.username.clone())
                .await
        {
            warn!(error = %e, "failed to ensure the activity actor exists");
        }
        for entry in entries {
            match self
                .db
                .record_list_activity(
                    list_id,
                    actor.user_id,
                    actor.username.clone(),
                    entry.kind,
                    entry.list_item_id,
                    entry.item_id,
                    entry.payload,
                    entry.message,
                )
                .await
            {
                Ok(activity) => {
                    let activity = ListActivity::from(activity);
                    if let Err(e) = self
                        .senders
                        .lists
                        .send(EventType::added(ListEventData::Activity(activity)))
                    {
                        warn!(error = %e, "failed to broadcast list activity");
                    }
                }
                Err(e) => warn!(error = %e, list_id, "failed to record list activity"),
            }
        }

        let origin_socket = match actor.origin {
            Origin::Socket(id) => Some(id),
            Origin::Rest | Origin::Bot => None,
        };
        if let Err(e) = self
            .senders
            .list_docs
            .send(EventType::Update(Arc::new(ListDocEvent {
                list_id,
                update: outcome.relay.clone(),
                origin_socket,
            })))
        {
            warn!(error = %e, "failed to relay list document update");
        }

        let meta_changed = outcome.meta_changed();
        Applied {
            relay: outcome.relay,
            changes: outcome.changes,
            meta_changed,
        }
    }
}
