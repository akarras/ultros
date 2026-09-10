//! Anonymous primary documents. Browser-only: never reuse an account cache key.
use super::adapter::{self, Edit};
use futures::lock::Mutex;
use leptos::prelude::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use ultros_list_doc::{ListDocument, ListUndo, MetaSnapshot, RowSnapshot, Subscription};
use wasm_bindgen::{JsCast, prelude::*};
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen(inline_js = r#"
const module = () => import('/static/guest-list-store.mjs');
let store;
async function db() { return store ||= module().then(m => m.openGuestListStore()).catch(e => {store = undefined; throw e;}); }
function announce(id) {
  window.dispatchEvent(new CustomEvent('ultros-device-list-change', {detail:id}));
  if (typeof BroadcastChannel !== 'undefined') { const c = new BroadcastChannel('ultros-device-lists'); c.postMessage(id); c.close(); }
}
export async function guestListRecords() { return JSON.stringify(await (await db()).list()); }
export async function guestLoad(id) { return (await db()).load(id); }
export async function guestCreate(name, snapshot) { const r = await (await db()).create({name, snapshot}); announce(r.id); return r; }
export async function guestSave(id, revision, name, snapshot) { const r = await (await db()).save(id, revision, {name, snapshot}); announce(id); return r; }
export async function guestRemove(id, revision) { await (await db()).remove(id, revision); announce(id); }
export async function guestEncode(name, snapshot) { return (await module()).encodeGuestListBackup({name, snapshot}); }
export async function guestDecode(text) { return (await module()).decodeGuestListBackup(text); }
export function guestWatch(id, callback) {
  const local = e => { if(e.detail === id) callback(); };
  const focus = () => callback();
  const channel = typeof BroadcastChannel !== 'undefined' ? new BroadcastChannel('ultros-device-lists') : null;
  if(channel) channel.onmessage = e => {if(e.data === id) callback();};
  window.addEventListener('ultros-device-list-change', local);
  window.addEventListener('focus', focus);
  return () => { channel?.close(); window.removeEventListener('ultros-device-list-change',local); window.removeEventListener('focus',focus); };
}
"#)]
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
    fn watch(id: &str, callback: &js_sys::Function) -> js_sys::Function;
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
fn error_message(error: JsValue) -> String {
    js_sys::Reflect::get(&error, &"message".into())
        .ok()
        .and_then(|v| v.as_string())
        .or_else(|| error.as_string())
        .unwrap_or_else(|| "Changes aren't saved on this device. Retry or export a backup.".into())
}
fn bytes(record: &JsValue) -> Result<Vec<u8>, String> {
    let value = property(record, "snapshot")?;
    if !value.is_instance_of::<js_sys::Uint8Array>() {
        return Err("The saved device list is damaged; it has not been removed.".into());
    }
    Ok(js_sys::Uint8Array::new(&value).to_vec())
}

type GuestWatcher = (js_sys::Function, Closure<dyn FnMut()>);

struct Inner {
    id: String,
    doc: ListDocument,
    undo: RefCell<ListUndo>,
    storage_revision: Cell<f64>,
    saved_version: RefCell<Vec<u8>>,
    lock: Mutex<()>,
    closed: Cell<bool>,
    _subscription: Subscription,
    watcher: RefCell<Option<GuestWatcher>>,
}
impl Drop for Inner {
    fn drop(&mut self) {
        if let Some((stop, _callback)) = self.watcher.get_mut().take() {
            let _ = stop.call0(&JsValue::NULL);
        }
    }
}

#[derive(Clone)]
pub struct GuestListHandle {
    inner: Rc<Inner>,
    pub revision: RwSignal<u64>,
    pub status: RwSignal<String>,
}
impl GuestListHandle {
    pub async fn list() -> Result<Vec<GuestListSummary>, String> {
        let json = JsFuture::from(records())
            .await
            .map_err(error_message)?
            .as_string()
            .ok_or("Invalid device list index")?;
        serde_json::from_str(&json).map_err(|e| e.to_string())
    }
    pub async fn create(name: &str) -> Result<Self, String> {
        let doc = ListDocument::new();
        doc.rename(name.trim()).map_err(|e| e.to_string())?;
        let snapshot = doc.export_snapshot().map_err(|e| e.to_string())?;
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
            return Err("This device list no longer exists.".into());
        }
        Self::from_record(record)
    }
    pub async fn restore(text: &str) -> Result<Self, String> {
        let decoded = JsFuture::from(decode(text)).await.map_err(error_message)?;
        let snapshot = bytes(&decoded)?;
        let doc = ListDocument::from_snapshot(&snapshot)
            .map_err(|e| format!("The backup document could not be read: {e}"))?;
        let name = property(&decoded, "name")?
            .as_string()
            .ok_or("The backup needs a name")?;
        doc.rename(&name).map_err(|e| e.to_string())?;
        let snapshot = doc.export_snapshot().map_err(|e| e.to_string())?;
        let record = JsFuture::from(create(
            &name,
            &js_sys::Uint8Array::from(snapshot.as_slice()),
        ))
        .await
        .map_err(error_message)?;
        Self::from_record(record)
    }
    fn from_record(record: JsValue) -> Result<Self, String> {
        let doc = ListDocument::from_snapshot(&bytes(&record)?).map_err(|e| {
            format!("The saved document could not be read; it has not been removed: {e}")
        })?;
        let revision = RwSignal::new(0u64);
        let subscription = doc.on_change(move || {
            let _ = revision.try_update(|n| *n += 1);
        });
        let handle = Self {
            revision,
            status: RwSignal::new("Saved on this device".into()),
            inner: Rc::new(Inner {
                id: property(&record, "id")?
                    .as_string()
                    .ok_or("Missing device identity")?,
                storage_revision: Cell::new(
                    property(&record, "revision")?
                        .as_f64()
                        .ok_or("Missing storage revision")?,
                ),
                saved_version: RefCell::new(doc.version()),
                undo: RefCell::new(ListUndo::new(&doc)),
                doc,
                lock: Mutex::new(()),
                closed: Cell::new(false),
                _subscription: subscription,
                watcher: RefCell::new(None),
            }),
        };
        let weak = Rc::downgrade(&handle.inner);
        let status = handle.status;
        let callback = Closure::<dyn FnMut()>::new(move || {
            if let Some(inner) = weak.upgrade() {
                let handle = Self {
                    inner,
                    revision,
                    status,
                };
                if !handle.inner.closed.get() {
                    handle.schedule_save();
                }
            }
        });
        let stop = watch(&handle.id(), callback.as_ref().unchecked_ref());
        *handle.inner.watcher.borrow_mut() = Some((stop, callback));
        Ok(handle)
    }
    pub fn id(&self) -> String {
        self.inner.id.clone()
    }
    pub fn storage_revision(&self) -> String {
        self.inner.storage_revision.get().to_string()
    }
    pub fn rows(&self) -> Vec<RowSnapshot> {
        self.inner.doc.rows()
    }
    pub fn meta(&self) -> MetaSnapshot {
        self.inner.doc.meta()
    }
    pub fn snapshot(&self) -> Result<Vec<u8>, String> {
        self.inner.doc.export_snapshot().map_err(|e| e.to_string())
    }
    pub fn apply(&self, edit: Edit) -> Result<(), String> {
        if self.inner.closed.get() {
            return Err("This list is closed. Reopen it to edit.".into());
        }
        adapter::apply(&self.inner.doc, &mut self.inner.undo.borrow_mut(), edit)
            .map_err(|e| e.to_string())?;
        self.schedule_save();
        Ok(())
    }
    pub fn rename(&self, name: &str) -> Result<(), String> {
        if self.inner.closed.get() {
            return Err("This list is closed. Reopen it to edit.".into());
        }
        if name.trim().is_empty() {
            return Err("Give this list a name.".into());
        }
        self.inner
            .undo
            .borrow_mut()
            .group(|| self.inner.doc.rename(name.trim()))
            .map_err(|e| e.to_string())?;
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
            Err(error) => {
                let _ = self.status.try_set(error.to_string());
                false
            }
        }
    }
    fn schedule_save(&self) {
        let handle = self.clone();
        if *self.inner.saved_version.borrow() != self.inner.doc.version() {
            let _ = self.status.try_set("Saving on this device…".into());
        }
        wasm_bindgen_futures::spawn_local(async move {
            let _ = handle.flush().await;
        });
    }
    /// Serializes this tab's saves, merging the latest transaction before each CAS.
    /// A racing tab gets merged and retried; never replace a missing/corrupt record.
    pub async fn flush(&self) -> Result<(), String> {
        let _guard = self.inner.lock.lock().await;
        let result = self.flush_locked().await;
        let _ = self.status.try_set(match &result {
            Ok(()) => "Saved on this device".into(),
            Err(e) => format!("Changes aren't saved on this device. {e}"),
        });
        result
    }
    async fn flush_locked(&self) -> Result<(), String> {
        for _ in 0..16 {
            let record = JsFuture::from(load(&self.id()))
                .await
                .map_err(error_message)?;
            if record.is_null() || record.is_undefined() {
                return Err(
                    "The list was removed in another window. Export to keep these edits.".into(),
                );
            }
            let stored_revision = property(&record, "revision")?
                .as_f64()
                .ok_or("Invalid storage revision")?;
            let dirty = *self.inner.saved_version.borrow() != self.inner.doc.version();
            if stored_revision != self.inner.storage_revision.get() {
                let report = self
                    .inner
                    .doc
                    .import(&bytes(&record)?)
                    .map_err(|e| e.to_string())?;
                if report.pending {
                    return Err(
                        "The saved document has incomplete history; export before retrying.".into(),
                    );
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
                            .ok_or("Invalid saved revision")?,
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
        Err("Other windows are still changing this list. Retry saving.".into())
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
        .ok_or_else(|| "Could not export the backup".into())
    }
    pub async fn remove(&self) -> Result<(), String> {
        if self.inner.closed.replace(true) {
            return Err("This list is already closed.".into());
        }
        let _guard = self.inner.lock.lock().await;
        let result = async {
            self.flush_locked().await?;
            JsFuture::from(remove(&self.id(), self.inner.storage_revision.get()))
                .await
                .map_err(error_message)?;
            Ok(())
        }
        .await;
        if result.is_ok() {
            self.stop_watch();
        } else {
            self.inner.closed.set(false);
        }
        result
    }
    fn stop_watch(&self) {
        if let Some((stop, _callback)) = self.inner.watcher.borrow_mut().take() {
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
