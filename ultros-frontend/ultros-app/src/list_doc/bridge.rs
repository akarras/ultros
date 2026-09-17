//! The device-list runtime behind `LocalLists`, so the shared add-to-list
//! modals (which live below this crate) can target lists saved on this
//! device. Each call opens the list, applies, waits for the IndexedDB save
//! and closes again: a modal never keeps a document open.
use leptos::prelude::*;
use ultros_api_types::list::ListItem;
use ultros_frontend_core::global_state::local_lists::{LocalListSummary, LocalLists, LocalResult};

use super::{adapter::Edit, guest::GuestListHandle};

fn list() -> LocalResult<Vec<LocalListSummary>> {
    Box::pin(async {
        Ok(GuestListHandle::list()
            .await?
            .into_iter()
            // A list bound to an account is already offered as an account
            // list (or is mid-transfer, where a write would race the
            // upload), and a record that failed to load cannot be opened.
            .filter(|list| list.online.is_none() && list.error.is_none())
            .map(|list| LocalListSummary {
                id: list.id,
                name: list.name,
            })
            .collect())
    })
}

fn create(name: String) -> LocalResult<LocalListSummary> {
    Box::pin(async move {
        let handle = GuestListHandle::create(&name).await?;
        let summary = LocalListSummary {
            id: handle.id(),
            name: handle.meta().name,
        };
        handle.close();
        Ok(summary)
    })
}

fn add_items(id: String, items: Vec<ListItem>) -> LocalResult<()> {
    Box::pin(async move {
        let handle = GuestListHandle::open(&id).await?;
        let result = async {
            handle.apply(Edit::AddMany(items))?;
            handle.flush().await
        }
        .await;
        handle.close();
        result
    })
}

pub fn provide_local_lists() {
    provide_context(LocalLists {
        list,
        create,
        add_items,
    });
}
