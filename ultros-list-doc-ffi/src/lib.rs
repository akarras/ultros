//! Prototype document/sync ABI. Document and response handles are checked integer IDs,
//! never Rust addresses. Responses are immutable: sizing/copying does not repeat an edit.
//! The caller owns its input/output buffers; no Rust allocation crosses the ABI.

#![recursion_limit = "256"]

#[cfg(panic = "abort")]
compile_error!("The prototype C ABI requires panic=unwind to contain Rust panics.");

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Mutex, OnceLock};

use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ultros_list_doc::{ListDocument, ListUndo, Quality, RowKey};
use ultros_list_doc::{SyncPayload, client_sync::ClientSync};

const MAX_SNAPSHOT: usize = 8 * 1024 * 1024;
const MAX_REQUEST: usize = 24 * 1024 * 1024;
const MAX_ROWS: usize = 256;
const MAX_DOCUMENTS: usize = 16;
const MAX_RESPONSES: usize = 64;
const MAX_QUANTITY: i64 = 1_000_000;

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Open {
        snapshot: Option<String>,
    },
    OpenSync {
        checkpoint: Option<OnlineCheckpoint>,
    },
    SyncPrepare {
        doc: u64,
    },
    SyncReceive {
        doc: u64,
        version: String,
        kind: String,
        payload: String,
    },
    SyncPersisted {
        doc: u64,
        version: String,
    },
    SyncError {
        doc: u64,
        code: String,
    },
    SyncCheckpoint {
        doc: u64,
    },
    SyncUseServer {
        doc: u64,
    },
    SyncRetry {
        doc: u64,
    },
    View {
        doc: u64,
    },
    Edit {
        doc: u64,
        edit: Edit,
    },
    Export {
        doc: u64,
    },
    Import {
        doc: u64,
        snapshot: String,
    },
    Close {
        doc: u64,
    },
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Edit {
    Add {
        item_id: i32,
        quality: String,
        need: i64,
    },
    SetNeed {
        item_id: i32,
        quality: String,
        need: i64,
    },
    AddAcquired {
        item_id: i32,
        quality: String,
        delta: i64,
    },
    SetAcquired {
        item_id: i32,
        quality: String,
        value: i64,
    },
    SetQuality {
        item_id: i32,
        quality: String,
        new_quality: String,
    },
    Remove {
        item_id: i32,
        quality: String,
    },
    Rename {
        name: String,
    },
    Undo,
    Redo,
}

struct Document {
    document: ListDocument,
    undo: ListUndo,
    sync: Option<ClientSync>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OnlineCheckpoint {
    format: u32,
    snapshot: String,
    state: ClientSync,
    review_server: Option<String>,
}

impl Document {
    fn new(document: ListDocument) -> Result<Self, String> {
        if document.meta().name.chars().count() > 128 {
            return Err("Snapshot name exceeds the prototype's 128-character limit".into());
        }
        let undo = ListUndo::new(&document);
        Ok(Self {
            document,
            undo,
            sync: None,
        })
    }

    fn view(&self) -> Value {
        json!({
            "name": self.document.meta().name,
            "peer_id": self.document.peer_id().to_string(),
            "can_undo": self.undo.can_undo(),
            "can_redo": self.undo.can_redo(),
            "version":STANDARD.encode(self.document.version()),
            "connected":self.sync.is_some(),
            "ready":self.sync.as_ref().is_none_or(|s|s.ready),
            "awaiting_persist":self.sync.as_ref().is_some_and(|s|s.awaiting_persist),
            "recovery_review":self.sync.as_ref().is_some_and(|s|s.review),
            "has_server_copy":self.sync.as_ref().is_some_and(|s|s.review_server.is_some()),
            "scope":self.document.meta().scope,
            "rows": self.document.rows().into_iter().map(|row| json!({
                "item_id": row.key.item_id, "quality": row.key.quality.as_str(),
                "need": row.need, "acquired": row.acquired, "target": row.target,
            })).collect::<Vec<_>>(),
        })
    }

    fn edit(&mut self, edit: Edit) -> Result<(), String> {
        if let Some(sync) = &self.sync {
            if !sync.can_edit() {
                return Err(
                    "Save local changes or review recovery before editing this list".into(),
                );
            }
            if matches!(edit, Edit::Rename { .. }) {
                return Err(
                    "The game connection can edit rows, not owner-only list metadata".into(),
                );
            }
        }
        let result = match edit {
            Edit::Add {
                item_id,
                quality,
                need,
            } => {
                let key = key(item_id, &quality)?;
                quantity(need, false)?;
                let next = self
                    .document
                    .row(&key)
                    .map_or(0, |r| r.need)
                    .checked_add(need)
                    .ok_or("Quantity overflow")?;
                quantity(next, false)?;
                if self.document.row(&key).is_none() && self.document.rows().len() >= MAX_ROWS {
                    return Err("Prototype row limit reached".into());
                }
                self.document.add_row(key, need, None)
            }
            Edit::SetNeed {
                item_id,
                quality,
                need,
            } => {
                quantity(need, false)?;
                self.document.set_need(&key(item_id, &quality)?, need)
            }
            Edit::AddAcquired {
                item_id,
                quality,
                delta,
            } => {
                quantity(delta, true)?;
                let key = key(item_id, &quality)?;
                if let Some(row) = self.document.row(&key) {
                    quantity(
                        row.acquired.checked_add(delta).ok_or("Quantity overflow")?,
                        true,
                    )?;
                }
                self.document.add_acquired(&key, delta)
            }
            Edit::SetAcquired {
                item_id,
                quality,
                value,
            } => {
                quantity(value, true)?;
                self.document.set_acquired(&key(item_id, &quality)?, value)
            }
            Edit::SetQuality {
                item_id,
                quality,
                new_quality,
            } => self
                .document
                .set_quality(&key(item_id, &quality)?, parse_quality(&new_quality)?)
                .map(|_| ()),
            Edit::Remove { item_id, quality } => self.document.remove_row(&key(item_id, &quality)?),
            Edit::Rename { name } => {
                if name.chars().count() > 128 || name.chars().any(char::is_control) {
                    return Err(
                        "Use a name of at most 128 characters without control characters".into(),
                    );
                }
                self.document.rename(&name)
            }
            Edit::Undo => self.undo.undo().map(|_| ()),
            Edit::Redo => self.undo.redo().map(|_| ()),
        };
        result.map_err(|e| e.to_string())?;
        if let Some(sync) = &mut self.sync {
            sync.local_changed();
        }
        Ok(())
    }
}

fn key(item_id: i32, quality: &str) -> Result<RowKey, String> {
    if !(1..=(i32::MAX - 2) / 4).contains(&item_id) {
        return Err("Invalid item ID".into());
    }
    Ok(RowKey {
        item_id,
        quality: parse_quality(quality)?,
    })
}

fn parse_quality(value: &str) -> Result<Quality, String> {
    value
        .parse()
        .map_err(|e: ultros_list_doc::KeyError| e.to_string())
}

fn quantity(value: i64, signed: bool) -> Result<(), String> {
    let min = if signed { -MAX_QUANTITY } else { 0 };
    if !(min..=MAX_QUANTITY).contains(&value) {
        return Err("Prototype quantities must be within one million".into());
    }
    Ok(())
}

fn validate_size(doc: &ListDocument) -> Result<(), String> {
    if doc.meta().name.chars().count() > 128 {
        return Err("Snapshot name exceeds the prototype's 128-character limit".into());
    }
    if doc.rows().len() > MAX_ROWS {
        return Err("Snapshot exceeds the prototype's 256-row limit".into());
    }
    Ok(())
}

fn decode(snapshot: &str) -> Result<Vec<u8>, String> {
    if snapshot.len() > MAX_SNAPSHOT.div_ceil(3) * 4 {
        return Err("Snapshot exceeds 8 MiB".into());
    }
    let bytes = STANDARD
        .decode(snapshot)
        .map_err(|_| "Invalid snapshot base64")?;
    if bytes.len() > MAX_SNAPSHOT {
        return Err("Snapshot exceeds 8 MiB".into());
    }
    Ok(bytes)
}

#[derive(Default)]
struct State {
    next_id: u64,
    documents: HashMap<u64, Document>,
    responses: HashMap<u64, Vec<u8>>,
}

impl State {
    fn id(&mut self) -> Result<u64, String> {
        self.next_id = self
            .next_id
            .checked_add(1)
            .filter(|n| *n <= i64::MAX as u64)
            .ok_or("Native handle space exhausted")?;
        Ok(self.next_id)
    }

    fn execute(&mut self, request: Request) -> Result<Value, String> {
        match request {
            Request::OpenSync { checkpoint } => {
                if self.documents.len() >= MAX_DOCUMENTS {
                    return Err("Prototype document limit reached".into());
                }
                let (document, mut sync) = if let Some(checkpoint) = checkpoint {
                    if checkpoint.format != 1 {
                        return Err(
                            "Incompatible online checkpoint; preserve the original file".into()
                        );
                    }
                    let document = ListDocument::from_snapshot(&decode(&checkpoint.snapshot)?)
                        .map_err(|e| e.to_string())?;
                    let mut sync = checkpoint.state;
                    sync.review_server =
                        checkpoint.review_server.map(|s| decode(&s)).transpose()?;
                    if let Some(server) = &sync.review_server {
                        ListDocument::from_snapshot(server).map_err(|e| e.to_string())?;
                    }
                    (document, sync)
                } else {
                    (ListDocument::empty_peer(), ClientSync::default())
                };
                sync.awaiting_persist = false;
                let undo = ListUndo::new(&document);
                let context = Document {
                    document,
                    undo,
                    sync: Some(sync),
                };
                let doc = self.id()?;
                let view = context.view();
                self.documents.insert(doc, context);
                Ok(json!({"ok":true,"doc":doc,"view":view}))
            }
            Request::SyncPrepare { doc } => {
                let context = self
                    .documents
                    .get_mut(&doc)
                    .ok_or("Document is closed or unknown")?;
                let sync = context.sync.as_mut().ok_or("Not an online document")?;
                let (version, update, requires_save) =
                    sync.prepare(&context.document).map_err(|e| e.to_string())?;
                Ok(
                    json!({"ok":true,"doc":doc,"view":context.view(),"request":{"version":STANDARD.encode(version),"update":update.map(|u|STANDARD.encode(u))},"requires_save":requires_save}),
                )
            }
            Request::SyncReceive {
                doc,
                version,
                kind,
                payload,
            } => {
                if version.len() > 65536 {
                    return Err("Invalid causal version".into());
                }
                let version = STANDARD
                    .decode(version)
                    .map_err(|_| "Invalid causal version")?;
                let payload = match kind.as_str() {
                    "snapshot" => SyncPayload::Snapshot(decode(&payload)?),
                    "updates" => SyncPayload::Updates(decode(&payload)?),
                    "up_to_date" if payload.is_empty() => SyncPayload::UpToDate,
                    _ => return Err("Unsupported sync payload".into()),
                };
                let context = self
                    .documents
                    .get_mut(&doc)
                    .ok_or("Document is closed or unknown")?;
                let sync = context.sync.as_mut().ok_or("Not an online document")?;
                let step = sync
                    .receive(&context.document, &version, payload)
                    .map_err(|e| e.to_string())?;
                if let Some(document) = step.replacement {
                    context.undo = ListUndo::new(&document);
                    context.document = document;
                }
                Ok(json!({"ok":true,"doc":doc,"view":context.view(),"status":step.status}))
            }
            Request::SyncPersisted { doc, version } => {
                let context = self
                    .documents
                    .get_mut(&doc)
                    .ok_or("Document is closed or unknown")?;
                if STANDARD.encode(context.document.version()) != version {
                    return Err("The saved document version is stale".into());
                }
                context
                    .sync
                    .as_mut()
                    .ok_or("Not an online document")?
                    .persisted();
                Ok(json!({"ok":true,"doc":doc,"view":context.view()}))
            }
            Request::SyncError { doc, code } => {
                let context = self
                    .documents
                    .get_mut(&doc)
                    .ok_or("Document is closed or unknown")?;
                context
                    .sync
                    .as_mut()
                    .ok_or("Not an online document")?
                    .error(&code);
                Ok(json!({"ok":true,"doc":doc,"view":context.view()}))
            }
            Request::SyncCheckpoint { doc } => {
                let context = self
                    .documents
                    .get(&doc)
                    .ok_or("Document is closed or unknown")?;
                let sync = context.sync.as_ref().ok_or("Not an online document")?;
                let checkpoint = OnlineCheckpoint {
                    format: 1,
                    snapshot: STANDARD.encode(
                        context
                            .document
                            .export_snapshot()
                            .map_err(|e| e.to_string())?,
                    ),
                    state: sync.clone(),
                    review_server: sync.review_server.as_ref().map(|s| STANDARD.encode(s)),
                };
                Ok(json!({"ok":true,"doc":doc,"checkpoint":checkpoint,"view":context.view()}))
            }
            Request::SyncUseServer { doc } => {
                let context = self
                    .documents
                    .get_mut(&doc)
                    .ok_or("Document is closed or unknown")?;
                let sync = context.sync.as_mut().ok_or("Not an online document")?;
                let bytes = sync
                    .review_server
                    .as_ref()
                    .ok_or("No preserved server copy")?;
                let document = ListDocument::from_snapshot(bytes).map_err(|e| e.to_string())?;
                let version = document.version();
                context.undo = ListUndo::new(&document);
                context.document = document;
                *sync = ClientSync {
                    ready: true,
                    server_version: Some(version),
                    awaiting_persist: true,
                    ..Default::default()
                };
                Ok(json!({"ok":true,"doc":doc,"view":context.view()}))
            }
            Request::SyncRetry { doc } => {
                let context = self
                    .documents
                    .get_mut(&doc)
                    .ok_or("Document is closed or unknown")?;
                let sync = context.sync.as_mut().ok_or("Not an online document")?;
                sync.review = false;
                sync.last_diff_version = None;
                sync.rebased_for = None;
                sync.force_snapshot = true;
                Ok(json!({"ok":true,"doc":doc,"view":context.view()}))
            }
            Request::Open { snapshot } => {
                if self.documents.len() >= MAX_DOCUMENTS {
                    return Err("Prototype document limit reached".into());
                }
                let document = match snapshot {
                    Some(bytes) => {
                        ListDocument::from_snapshot(&decode(&bytes)?).map_err(|e| e.to_string())?
                    }
                    None => {
                        let d = ListDocument::new();
                        d.rename("Prototype cart").map_err(|e| e.to_string())?;
                        d
                    }
                };
                let document = Document::new(document)?;
                let doc = self.id()?;
                let view = document.view();
                self.documents.insert(doc, document);
                Ok(json!({ "ok": true, "doc": doc, "view": view }))
            }
            Request::Close { doc } => {
                self.documents.remove(&doc);
                Ok(json!({ "ok": true }))
            }
            other => {
                let id = match &other {
                    Request::View { doc }
                    | Request::Edit { doc, .. }
                    | Request::Export { doc }
                    | Request::Import { doc, .. } => *doc,
                    _ => unreachable!(),
                };
                let context = self
                    .documents
                    .get_mut(&id)
                    .ok_or("Document is closed or unknown")?;
                match other {
                    Request::Edit { edit, .. } => context.edit(edit)?,
                    Request::Import { snapshot, .. } => {
                        let bytes = decode(&snapshot)?;
                        // Preflight resource bounds on an isolated peer before touching live state.
                        let candidate = ListDocument::from_snapshot(
                            &context
                                .document
                                .export_snapshot()
                                .map_err(|e| e.to_string())?,
                        )
                        .map_err(|e| e.to_string())?;
                        if candidate.import(&bytes).map_err(|e| e.to_string())?.pending {
                            return Err("Snapshot needs history this peer does not have".into());
                        }
                        validate_size(&candidate)?;
                        context.document.import(&bytes).map_err(|e| e.to_string())?;
                    }
                    Request::Export { .. } => {
                        let bytes = context
                            .document
                            .export_snapshot()
                            .map_err(|e| e.to_string())?;
                        if bytes.len() > MAX_SNAPSHOT {
                            return Err("Snapshot exceeds 8 MiB".into());
                        }
                        return Ok(json!({ "ok": true, "snapshot": STANDARD.encode(bytes) }));
                    }
                    _ => {}
                }
                Ok(json!({ "ok": true, "doc": id, "view": context.view() }))
            }
        }
    }

    fn request(&mut self, bytes: &[u8]) -> u64 {
        // Reserve response capacity BEFORE executing a mutation.
        if self.responses.len() >= MAX_RESPONSES {
            return 0;
        }
        let Ok(response) = self.id() else {
            return 0;
        };
        let value = serde_json::from_slice::<Request>(bytes)
            .map_err(|e| format!("Invalid request: {e}"))
            .and_then(|r| self.execute(r))
            .unwrap_or_else(|error| json!({ "ok": false, "error": error }));
        let Ok(encoded) = serde_json::to_vec(&value) else {
            return 0;
        };
        self.responses.insert(response, encoded);
        response
    }
}

fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(State::default()))
}

#[unsafe(no_mangle)]
pub extern "C" fn ultros_list_proto_abi() -> u32 {
    2
}

/// Idempotent close without allocating a response, including from SafeHandle finalizers.
#[unsafe(no_mangle)]
pub extern "C" fn ultros_list_proto_close(doc: u64) -> u32 {
    catch_unwind(|| {
        state().lock().map_or(1, |mut s| {
            s.documents.remove(&doc);
            0
        })
    })
    .unwrap_or(1)
}

/// Execute one UTF-8 JSON request; return an immutable response ID, or 0 on a
/// boundary failure. A poisoned/panicked registry is not reused: restart the host.
/// # Safety
/// For nonzero lengths within the request limit, `bytes` must point to `length`
/// readable bytes for this call. Null/oversized inputs are rejected before access.
/// The caller retains ownership and must not mutate concurrently.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ultros_list_proto_request(bytes: *const u8, length: usize) -> u64 {
    if length > MAX_REQUEST || (bytes.is_null() && length != 0) {
        return 0;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let input = if length == 0 {
            &[]
        } else {
            // SAFETY: the caller guarantees a live buffer; length/null are bounded above.
            unsafe { std::slice::from_raw_parts(bytes, length) }
        };
        state().lock().map_or(0, |mut s| s.request(input))
    }))
    .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn ultros_list_proto_response_len(response: u64) -> usize {
    catch_unwind(|| {
        state()
            .lock()
            .ok()
            .and_then(|s| s.responses.get(&response).map(Vec::len))
            .unwrap_or(0)
    })
    .unwrap_or(0)
}

/// Copy a response without repeating the request. Codes: 0 success, 1 unknown,
/// 2 insufficient capacity, 3 invalid buffer/boundary failure. No partial copies.
/// # Safety
/// `output` must point to at least `capacity` writable bytes for the call and
/// must not alias Rust-owned data. The caller retains ownership.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ultros_list_proto_response_copy(
    response: u64,
    output: *mut u8,
    capacity: usize,
) -> u32 {
    if output.is_null() {
        return 3;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let Ok(s) = state().lock() else {
            return 3;
        };
        let Some(bytes) = s.responses.get(&response) else {
            return 1;
        };
        if capacity < bytes.len() {
            return 2;
        }
        // SAFETY: caller supplies nonaliasing writable capacity; the mutex pins this
        // immutable response until the copy finishes, including against concurrent free.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), output, bytes.len());
        }
        0
    }))
    .unwrap_or(3)
}

#[unsafe(no_mangle)]
pub extern "C" fn ultros_list_proto_response_free(response: u64) {
    let _ = catch_unwind(|| {
        if let Ok(mut s) = state().lock() {
            s.responses.remove(&response);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(s: &mut State, request: Value) -> Value {
        let response = s.request(&serde_json::to_vec(&request).unwrap());
        let bytes = s.responses.remove(&response).expect("response");
        serde_json::from_slice(&bytes).unwrap()
    }
    fn open(s: &mut State, snapshot: Option<&Value>) -> u64 {
        call(s, json!({"op":"open", "snapshot":snapshot}))["doc"]
            .as_u64()
            .unwrap()
    }
    fn snapshot(s: &mut State, doc: u64) -> Value {
        call(s, json!({"op":"export","doc":doc}))["snapshot"].clone()
    }
    fn edit(s: &mut State, doc: u64, edit: Value) -> Value {
        call(s, json!({"op":"edit","doc":doc,"edit":edit}))
    }

    #[test]
    fn independent_peers_merge_acquisitions_and_duplicate_imports_once() {
        let mut s = State::default();
        let a = open(&mut s, None);
        assert_eq!(
            edit(
                &mut s,
                a,
                json!({"kind":"add","item_id":8,"quality":"nq","need":10})
            )["ok"],
            true
        );
        let initial = snapshot(&mut s, a);
        let b = open(&mut s, Some(&initial));
        assert_ne!(
            s.documents[&a].document.peer_id(),
            s.documents[&b].document.peer_id()
        );
        edit(
            &mut s,
            a,
            json!({"kind":"add_acquired","item_id":8,"quality":"nq","delta":2}),
        );
        edit(
            &mut s,
            b,
            json!({"kind":"add_acquired","item_id":8,"quality":"nq","delta":3}),
        );
        let from_b = snapshot(&mut s, b);
        for _ in 0..3 {
            assert_eq!(
                call(&mut s, json!({"op":"import","doc":a,"snapshot":from_b}))["ok"],
                true
            );
        }
        let from_a = snapshot(&mut s, a);
        call(&mut s, json!({"op":"import","doc":b,"snapshot":from_a}));
        assert_eq!(s.documents[&a].document.rows()[0].acquired, 5);
        assert_eq!(
            s.documents[&a].document.rows(),
            s.documents[&b].document.rows()
        );
    }

    #[test]
    fn malformed_requests_and_stale_handles_do_not_change_live_document() {
        let mut s = State::default();
        let a = open(&mut s, None);
        let before = s.documents[&a].document.version();
        assert_eq!(
            call(&mut s, json!({"op":"import","doc":a,"snapshot":"bad!"}))["ok"],
            false
        );
        assert_eq!(
            edit(
                &mut s,
                a,
                json!({"kind":"add","item_id":-1,"quality":"any","need":2})
            )["ok"],
            false
        );
        assert_eq!(before, s.documents[&a].document.version());
        assert_eq!(call(&mut s, json!({"op":"view","doc":9999}))["ok"], false);
        call(&mut s, json!({"op":"close","doc":a}));
        assert_eq!(call(&mut s, json!({"op":"view","doc":a}))["ok"], false);
    }

    #[test]
    fn full_response_pool_rejects_before_mutating() {
        let mut s = State::default();
        let a = open(&mut s, None);
        let before = s.documents[&a].document.version();
        for _ in 0..MAX_RESPONSES {
            assert_ne!(s.request(br#"{"op":"view","doc":0}"#), 0);
        }
        assert_eq!(
            s.request(
                &serde_json::to_vec(
                    &json!({"op":"edit","doc":a,"edit":{"kind":"rename","name":"changed"}})
                )
                .unwrap()
            ),
            0
        );
        assert_eq!(s.documents[&a].document.version(), before);
    }

    #[test]
    fn document_limit_and_close_do_not_reuse_stale_ids() {
        let mut s = State::default();
        let first = open(&mut s, None);
        for _ in 1..MAX_DOCUMENTS {
            open(&mut s, None);
        }
        assert_eq!(call(&mut s, json!({"op":"open"}))["ok"], false);
        assert_eq!(s.documents.len(), MAX_DOCUMENTS);
        call(&mut s, json!({"op":"close","doc":first}));
        call(&mut s, json!({"op":"close","doc":first}));
        let replacement = open(&mut s, None);
        assert_ne!(first, replacement);
        assert_eq!(call(&mut s, json!({"op":"view","doc":first}))["ok"], false);
        assert_eq!(s.documents.len(), MAX_DOCUMENTS);
    }

    #[test]
    fn oversized_projection_import_is_rejected_before_mutating_live_state() {
        let mut s = State::default();
        let a = open(&mut s, None);
        let before = s.documents[&a].document.version();
        let branch =
            ListDocument::from_snapshot(&s.documents[&a].document.export_snapshot().unwrap())
                .unwrap();
        for item_id in 1..=MAX_ROWS as i32 + 1 {
            branch
                .add_row(key(item_id, "nq").unwrap(), 1, None)
                .unwrap();
        }
        let too_many_rows = STANDARD.encode(branch.export_snapshot().unwrap());
        assert_eq!(
            call(
                &mut s,
                json!({"op":"import","doc":a,"snapshot":too_many_rows})
            )["ok"],
            false
        );
        assert_eq!(before, s.documents[&a].document.version());
        assert!(s.documents[&a].document.rows().is_empty());
        assert_eq!(
            call(&mut s, json!({"op":"open","snapshot":too_many_rows}))["ok"],
            true
        );
        // Existing merged/undo state may exceed the soft add/import limit and
        // must remain reopenable so the player can remove rows.
        assert_eq!(s.documents.len(), 2);
    }

    #[test]
    fn undo_crossing_the_soft_row_limit_remains_exportable_and_reopenable() {
        let mut s = State::default();
        let seed = ListDocument::new();
        for id in 1..=MAX_ROWS as i32 {
            seed.add_row(RowKey::new(id, None), 1, None).unwrap();
        }
        let seed_bytes = json!(STANDARD.encode(seed.export_snapshot().unwrap()));
        let a = open(&mut s, Some(&seed_bytes));
        edit(
            &mut s,
            a,
            json!({"kind":"remove","item_id":1,"quality":"any"}),
        );
        let branch_bytes = snapshot(&mut s, a);
        let b = open(&mut s, Some(&branch_bytes));
        edit(
            &mut s,
            b,
            json!({"kind":"add","item_id":257,"quality":"any","need":1}),
        );
        let branch = STANDARD.encode(s.documents[&b].document.export_snapshot().unwrap());
        assert_eq!(
            call(&mut s, json!({"op":"import","doc":a,"snapshot":branch}))["ok"],
            true
        );
        edit(&mut s, a, json!({"kind":"undo"}));
        assert_eq!(s.documents[&a].document.rows().len(), 257);
        let saved = call(&mut s, json!({"op":"export","doc":a}))["snapshot"]
            .as_str()
            .unwrap()
            .to_string();
        let reopened = open(&mut s, Some(&json!(saved)));
        assert_eq!(s.documents[&reopened].document.rows().len(), 257);
        assert_eq!(
            call(
                &mut s,
                json!({"op":"edit","doc":reopened,"edit":{"kind":"add","item_id":258,"quality":"any","need":1}})
            )["ok"],
            false
        );
        edit(
            &mut s,
            reopened,
            json!({"kind":"remove","item_id":1,"quality":"any"}),
        );
        assert_eq!(s.documents[&reopened].document.rows().len(), 256);
    }

    #[test]
    fn online_checkpoint_fences_edits_and_preserves_the_original_operations_after_restart() {
        let mut s = State::default();
        let server = ListDocument::new();
        server.rename("Online").unwrap();
        server.add_row(RowKey::new(8, None), 10, None).unwrap();
        let opened = call(&mut s, json!({"op":"open_sync"}));
        let a = opened["doc"].as_u64().unwrap();
        assert_eq!(
            call(
                &mut s,
                json!({"op":"edit","doc":a,"edit":{"kind":"add_acquired","item_id":8,"quality":"any","delta":2}})
            )["ok"],
            false
        );
        let reply = call(
            &mut s,
            json!({"op":"sync_receive","doc":a,"version":STANDARD.encode(server.version()),"kind":"snapshot","payload":STANDARD.encode(server.export_snapshot().unwrap())}),
        );
        assert_eq!(reply["ok"], true);
        assert_eq!(reply["view"]["awaiting_persist"], true);
        assert_eq!(
            call(&mut s, json!({"op":"sync_prepare","doc":a}))["ok"],
            false
        );
        assert_eq!(
            call(
                &mut s,
                json!({"op":"sync_persisted","doc":a,"version":"stale"})
            )["ok"],
            false
        );
        call(
            &mut s,
            json!({"op":"sync_persisted","doc":a,"version":reply["view"]["version"]}),
        );
        edit(
            &mut s,
            a,
            json!({"kind":"add_acquired","item_id":8,"quality":"any","delta":2}),
        );
        let version = STANDARD.encode(s.documents[&a].document.version());
        call(
            &mut s,
            json!({"op":"sync_persisted","doc":a,"version":version}),
        );
        let first = call(&mut s, json!({"op":"sync_prepare","doc":a}));
        assert_eq!(first["requires_save"], true);
        let checkpoint =
            call(&mut s, json!({"op":"sync_checkpoint","doc":a}))["checkpoint"].clone();
        let restored = call(&mut s, json!({"op":"open_sync","checkpoint":checkpoint}))["doc"]
            .as_u64()
            .unwrap();
        let retry = call(&mut s, json!({"op":"sync_prepare","doc":restored}));
        assert_eq!(retry["request"]["update"], first["request"]["update"]);
        for payload in [&first, &retry] {
            server
                .import(
                    &STANDARD
                        .decode(payload["request"]["update"].as_str().unwrap())
                        .unwrap(),
                )
                .unwrap();
        }
        assert_eq!(server.rows()[0].acquired, 2);
    }

    #[test]
    fn abi_copy_is_repeatable_and_never_writes_past_capacity() {
        let request = br#"{"op":"open"}"#;
        // SAFETY: null/oversized inputs are rejected before access; every other
        // pointer below is backed by a live allocation of the supplied length.
        unsafe {
            assert_eq!(ultros_list_proto_request(std::ptr::null(), 1), 0);
            assert_eq!(
                ultros_list_proto_request(request.as_ptr(), MAX_REQUEST + 1),
                0
            );
            let response = ultros_list_proto_request(request.as_ptr(), request.len());
            let len = ultros_list_proto_response_len(response);
            let mut bytes = vec![0xCC; len + 4];
            assert_eq!(
                ultros_list_proto_response_copy(response, bytes.as_mut_ptr(), len - 1),
                2
            );
            assert!(bytes.iter().all(|v| *v == 0xCC));
            assert_eq!(
                ultros_list_proto_response_copy(response, bytes.as_mut_ptr(), len),
                0
            );
            let first = bytes.clone();
            assert_eq!(
                ultros_list_proto_response_copy(response, bytes.as_mut_ptr(), len),
                0
            );
            assert_eq!(first, bytes);
            assert_eq!(&bytes[len..], &[0xCC; 4]);
            let parsed: Value = serde_json::from_slice(&bytes[..len]).unwrap();
            let close = serde_json::to_vec(&json!({"op":"close","doc":parsed["doc"]})).unwrap();
            let closed = ultros_list_proto_request(close.as_ptr(), close.len());
            ultros_list_proto_response_free(closed);
            ultros_list_proto_response_free(response);
            ultros_list_proto_response_free(response);
            assert_eq!(ultros_list_proto_response_len(response), 0);
            assert_eq!(
                ultros_list_proto_response_copy(response, bytes.as_mut_ptr(), len),
                1
            );
        }
    }
}
