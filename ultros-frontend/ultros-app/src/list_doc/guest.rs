//! Anonymous primary documents. Browser-only: never reuse an account cache key.
use super::adapter::{self, Edit};
use crate::i18n::*;
use futures::lock::Mutex;
use leptos::prelude::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use ultros_list_doc::{ListDocument, ListUndo, MetaSnapshot, RowSnapshot, Subscription};
use wasm_bindgen::{JsCast, prelude::*};
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen(module = "/../../ultros/static/guest-list-store.mjs")]
extern "C" {
    #[wasm_bindgen(js_name = guestListRecords)]
    fn records() -> js_sys::Promise;
    #[wasm_bindgen(js_name = guestLoad)]
    fn load(id: &str) -> js_sys::Promise;
    #[wasm_bindgen(js_name = guestCreate)]
    fn create(name: &str, snapshot: &js_sys::Uint8Array) -> js_sys::Promise;
    #[wasm_bindgen(js_name = guestSave)]
    fn save(id: &str, revision: f64, name: &str, snapshot: &js_sys::Uint8Array) -> js_sys::Promise;
    #[wasm_bindgen(js_name = guestRemove)]
    fn remove(id: &str, revision: f64) -> js_sys::Promise;
    #[wasm_bindgen(js_name = guestEncode)]
    fn encode(name: &str, snapshot: &js_sys::Uint8Array) -> js_sys::Promise;
    #[wasm_bindgen(js_name = guestDecode)]
    fn decode(text: &str) -> js_sys::Promise;
    #[wasm_bindgen(js_name = guestWatch)]
    fn watch(
        id: &str,
        revision: &js_sys::Function,
        callback: &js_sys::Function,
    ) -> js_sys::Function;
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct GuestListSummary {
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// IndexedDB revision used to distinguish a transferred snapshot from
    /// device edits made after its acknowledgement. Corrupt records use zero.
    #[serde(default)]
    pub revision: u64,
    pub error: Option<String>,
}

fn property(value: &JsValue, key: &str) -> Result<JsValue, String> {
    js_sys::Reflect::get(value, &key.into()).map_err(error_message)
}
// Resolve from the document so async persistence callbacks do not depend on a
// still-live reactive owner. Locale changes update the document language.
fn message(code: &str) -> String {
    let locale = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
        .and_then(|e| e.get_attribute("lang"))
        .and_then(|lang| lang.parse::<Locale>().ok())
        .unwrap_or_default();
    match code {
        "saved" => td_string!(locale, device_runtime_saved).to_string(),
        "saving" => td_string!(locale, device_runtime_saving).to_string(),
        "corrupt" => td_string!(locale, device_runtime_corrupt).to_string(),
        "missing" => td_string!(locale, device_runtime_missing).to_string(),
        "closed" => td_string!(locale, device_runtime_closed).to_string(),
        "name" => td_string!(locale, device_runtime_name).to_string(),
        "removed" => td_string!(locale, device_runtime_removed).to_string(),
        "history" => td_string!(locale, device_runtime_history).to_string(),
        "conflict" => td_string!(locale, device_runtime_conflict).to_string(),
        "backup" => td_string!(locale, device_runtime_backup).to_string(),
        "invalid" => td_string!(locale, device_runtime_invalid).to_string(),
        "unavailable" => td_string!(locale, device_runtime_unavailable).to_string(),
        "blocked" => td_string!(locale, device_runtime_blocked).to_string(),
        "QuotaExceededError" => td_string!(locale, device_runtime_quota).to_string(),
        _ => td_string!(locale, device_runtime_unsaved).to_string(),
    }
}
fn error_message(error: JsValue) -> String {
    let field = |key: &str| {
        js_sys::Reflect::get(&error, &key.into())
            .ok()
            .and_then(|v| v.as_string())
    };
    message(
        field("code")
            .or_else(|| field("name"))
            .as_deref()
            .unwrap_or("unsaved"),
    )
}
fn bytes(record: &JsValue) -> Result<Vec<u8>, String> {
    let value = property(record, "snapshot")?;
    if !value.is_instance_of::<js_sys::Uint8Array>() {
        return Err(message("corrupt"));
    }
    Ok(js_sys::Uint8Array::new(&value).to_vec())
}

type GuestWatcher = (
    js_sys::Function,
    Closure<dyn FnMut()>,
    Closure<dyn FnMut() -> f64>,
);

struct Inner {
    id: String,
    doc: ListDocument,
    undo: RefCell<ListUndo>,
    storage_revision: Cell<f64>,
    saved_version: RefCell<Vec<u8>>,
    lock: Mutex<()>,
    closed: Cell<bool>,
    removed: Cell<bool>,
    _subscription: Subscription,
    watcher: RefCell<Option<GuestWatcher>>,
}
impl Drop for Inner {
    fn drop(&mut self) {
        if let Some((stop, _callback, _revision)) = self.watcher.get_mut().take() {
            let _ = stop.call0(&JsValue::NULL);
        }
    }
}

#[derive(Clone)]
pub struct GuestListHandle {
    inner: Rc<Inner>,
    pub revision: RwSignal<u64>,
    pub status: RwSignal<String>,
    saving: RwSignal<bool>,
    save_failed: RwSignal<bool>,
}
impl GuestListHandle {
    pub async fn list() -> Result<Vec<GuestListSummary>, String> {
        let json = JsFuture::from(records())
            .await
            .map_err(error_message)?
            .as_string()
            .ok_or_else(|| message("corrupt"))?;
        let mut records: Vec<GuestListSummary> =
            serde_json::from_str(&json).map_err(|_| message("corrupt"))?;
        for record in &mut records {
            if let Some(code) = &record.error {
                record.error = Some(message(code));
            }
        }
        Ok(records)
    }
    pub async fn create(name: &str) -> Result<Self, String> {
        let doc = ListDocument::new();
        doc.rename(name.trim()).map_err(|_| message("invalid"))?;
        let snapshot = doc.export_snapshot().map_err(|_| message("invalid"))?;
        let record = JsFuture::from(create(
            name.trim(),
            &js_sys::Uint8Array::from(snapshot.as_slice()),
        ))
        .await
        .map_err(error_message)?;
        Self::from_record(record)
    }
    pub async fn open(id: &str) -> Result<Self, String> {
        let record = JsFuture::from(load(id)).await.map_err(error_message)?;
        if record.is_null() || record.is_undefined() {
            return Err(message("missing"));
        }
        Self::from_record(record)
    }
    pub async fn restore(text: &str) -> Result<Self, String> {
        let decoded = JsFuture::from(decode(text)).await.map_err(error_message)?;
        let snapshot = bytes(&decoded)?;
        let doc = ListDocument::from_snapshot(&snapshot).map_err(|_| message("backup"))?;
        let name = property(&decoded, "name")?
            .as_string()
            .ok_or_else(|| message("corrupt"))?;
        doc.rename(&name).map_err(|_| message("invalid"))?;
        let snapshot = doc.export_snapshot().map_err(|_| message("invalid"))?;
        let record = JsFuture::from(create(
            &name,
            &js_sys::Uint8Array::from(snapshot.as_slice()),
        ))
        .await
        .map_err(error_message)?;
        Self::from_record(record)
    }
    fn from_record(record: JsValue) -> Result<Self, String> {
        let doc = ListDocument::from_snapshot(&bytes(&record)?).map_err(|_| message("corrupt"))?;
        let revision = RwSignal::new(0u64);
        let subscription = doc.on_change(move || {
            let _ = revision.try_update(|n| *n += 1);
        });
        let handle = Self {
            revision,
            status: RwSignal::new(message("saved")),
            saving: RwSignal::new(false),
            save_failed: RwSignal::new(false),
            inner: Rc::new(Inner {
                id: property(&record, "id")?
                    .as_string()
                    .ok_or_else(|| message("corrupt"))?,
                storage_revision: Cell::new(
                    property(&record, "revision")?
                        .as_f64()
                        .ok_or_else(|| message("corrupt"))?,
                ),
                saved_version: RefCell::new(doc.version()),
                undo: RefCell::new(ListUndo::new(&doc)),
                doc,
                lock: Mutex::new(()),
                closed: Cell::new(false),
                removed: Cell::new(false),
                _subscription: subscription,
                watcher: RefCell::new(None),
            }),
        };
        let weak = Rc::downgrade(&handle.inner);
        let status = handle.status;
        let saving = handle.saving;
        let save_failed = handle.save_failed;
        let callback = Closure::<dyn FnMut()>::new(move || {
            if let Some(inner) = weak.upgrade() {
                let handle = Self {
                    inner,
                    revision,
                    status,
                    saving,
                    save_failed,
                };
                if !handle.inner.closed.get() {
                    handle.schedule_save();
                }
            }
        });
        let weak = Rc::downgrade(&handle.inner);
        let stored_revision = Closure::<dyn FnMut() -> f64>::new(move || {
            weak.upgrade()
                .map(|inner| inner.storage_revision.get())
                .unwrap_or(0.0)
        });
        let stop = watch(
            &handle.id(),
            stored_revision.as_ref().unchecked_ref(),
            callback.as_ref().unchecked_ref(),
        );
        *handle.inner.watcher.borrow_mut() = Some((stop, callback, stored_revision));
        Ok(handle)
    }
    pub fn id(&self) -> String {
        self.inner.id.clone()
    }
    pub fn storage_revision(&self) -> String {
        self.inner.storage_revision.get().to_string()
    }
    pub fn needs_save_retry(&self) -> bool {
        self.save_failed.get() && !self.is_saving()
    }
    pub fn is_saving(&self) -> bool {
        self.saving.get()
    }
    pub fn is_saved(&self) -> bool {
        self.revision.track();
        self.status.track();
        !self.save_failed.get() && *self.inner.saved_version.borrow() == self.inner.doc.version()
    }
    pub fn rows(&self) -> Vec<RowSnapshot> {
        self.inner.doc.rows()
    }
    pub fn meta(&self) -> MetaSnapshot {
        self.inner.doc.meta()
    }
    pub fn snapshot(&self) -> Result<Vec<u8>, String> {
        self.inner
            .doc
            .export_snapshot()
            .map_err(|_| message("invalid"))
    }
    pub fn apply(&self, edit: Edit) -> Result<(), String> {
        if self.inner.closed.get() {
            return Err(message("closed"));
        }
        adapter::apply(&self.inner.doc, &mut self.inner.undo.borrow_mut(), edit)
            .map_err(|_| message("invalid"))?;
        self.schedule_save();
        Ok(())
    }
    pub fn rename(&self, name: &str) -> Result<(), String> {
        if self.inner.closed.get() {
            return Err(message("closed"));
        }
        if name.trim().is_empty() {
            return Err(message("name"));
        }
        self.inner
            .undo
            .borrow_mut()
            .group(|| self.inner.doc.rename(name.trim()))
            .map_err(|_| message("invalid"))?;
        self.schedule_save();
        Ok(())
    }
    pub fn undo(&self) -> bool {
        self.history(false)
    }
    pub fn redo(&self) -> bool {
        self.history(true)
    }
    fn history(&self, redo: bool) -> bool {
        if self.inner.closed.get() {
            return false;
        }
        let result = if redo {
            self.inner.undo.borrow_mut().redo()
        } else {
            self.inner.undo.borrow_mut().undo()
        };
        match result {
            Ok(changed) => {
                if changed {
                    self.schedule_save();
                }
                changed
            }
            Err(_) => {
                let _ = self.status.try_set(message("invalid"));
                false
            }
        }
    }
    fn schedule_save(&self) {
        let handle = self.clone();
        if *self.inner.saved_version.borrow() != self.inner.doc.version() {
            let _ = self.saving.try_set(true);
            let _ = self.status.try_set(message("saving"));
        }
        wasm_bindgen_futures::spawn_local(async move {
            let _ = handle.flush().await;
        });
    }
    /// Serializes this tab's saves, merging the latest transaction before each CAS.
    /// A racing tab gets merged and retried; never replace a missing/corrupt record.
    pub async fn flush(&self) -> Result<(), String> {
        let _guard = self.inner.lock.lock().await;
        let _ = self.saving.try_set(true);
        let result = self.flush_locked().await;
        let _ = self.saving.try_set(false);
        let _ = self.save_failed.try_set(result.is_err());
        let _ = self.status.try_set(match &result {
            Ok(()) => message("saved"),
            Err(e) => format!("{} {e}", message("unsaved")),
        });
        result
    }
    async fn flush_locked(&self) -> Result<(), String> {
        if self.inner.removed.get() {
            return Ok(());
        }
        for _ in 0..16 {
            let record = JsFuture::from(load(&self.id()))
                .await
                .map_err(error_message)?;
            if record.is_null() || record.is_undefined() {
                return Err(message("removed"));
            }
            let stored_revision = property(&record, "revision")?
                .as_f64()
                .ok_or_else(|| message("corrupt"))?;
            let dirty = *self.inner.saved_version.borrow() != self.inner.doc.version();
            if stored_revision != self.inner.storage_revision.get() {
                let report = self
                    .inner
                    .doc
                    .import(&bytes(&record)?)
                    .map_err(|_| message("invalid"))?;
                if report.pending {
                    return Err(message("history"));
                }
                self.inner.storage_revision.set(stored_revision);
                if !dirty {
                    *self.inner.saved_version.borrow_mut() = self.inner.doc.version();
                }
            }
            if !dirty {
                return Ok(());
            }
            let version = self.inner.doc.version();
            let snapshot = self.snapshot()?;
            let name = self.meta().name;
            match JsFuture::from(save(
                &self.id(),
                stored_revision,
                &name,
                &js_sys::Uint8Array::from(snapshot.as_slice()),
            ))
            .await
            {
                Ok(saved) => {
                    self.inner.storage_revision.set(
                        property(&saved, "revision")?
                            .as_f64()
                            .ok_or_else(|| message("corrupt"))?,
                    );
                    *self.inner.saved_version.borrow_mut() = version;
                    if *self.inner.saved_version.borrow() == self.inner.doc.version() {
                        return Ok(());
                    }
                }
                Err(error) => {
                    if property(&error, "code")
                        .ok()
                        .and_then(|v| v.as_string())
                        .as_deref()
                        != Some("conflict")
                    {
                        return Err(error_message(error));
                    }
                }
            }
        }
        Err(message("conflict"))
    }
    pub async fn backup(&self) -> Result<String, String> {
        let snapshot = self.snapshot()?;
        JsFuture::from(encode(
            &self.meta().name,
            &js_sys::Uint8Array::from(snapshot.as_slice()),
        ))
        .await
        .map_err(error_message)?
        .as_string()
        .ok_or_else(|| message("backup"))
    }
    pub async fn remove(&self) -> Result<(), String> {
        let was_closed = self.inner.closed.replace(true);
        let _guard = self.inner.lock.lock().await;
        if self.inner.removed.get() {
            return Ok(());
        }
        let result = async {
            self.flush_locked().await?;
            JsFuture::from(remove(&self.id(), self.inner.storage_revision.get()))
                .await
                .map_err(error_message)?;
            Ok(())
        }
        .await;
        if result.is_ok() {
            self.inner.removed.set(true);
            self.stop_watch();
        } else {
            self.inner.closed.set(was_closed);
        }
        result
    }
    fn stop_watch(&self) {
        if let Some((stop, _callback, _revision)) = self.inner.watcher.borrow_mut().take() {
            let _ = stop.call0(&JsValue::NULL);
        }
    }
    pub fn close(&self) {
        if !self.inner.closed.replace(true) {
            self.stop_watch();
            self.schedule_save();
        }
    }
}
