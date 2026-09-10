// Durable primary storage for anonymous Lists 2.0 documents. This is separate
// from the account-scoped localStorage cache and never evicts lists.
//
// Snapshots are opaque Loro bytes; callers must validate/merge them with
// ListDocument. Compare-and-swap prevents stale tabs overwriting newer data:
// on conflict, load, merge the latest snapshot, and retry its revision.
// Promise success means the IndexedDB transaction committed, not just that
// an individual request succeeded. No account credentials or IDs belong here.

const DATABASE = "ultros-device-lists-v1";
const STORE = "lists";
const BACKUP_FORMAT = "ultros-device-list";

export class GuestListStoreError extends Error {
  constructor(code, message) {
    super(message);
    this.name = "GuestListStoreError";
    this.code = code;
  }
}

function fail(code, message) {
  throw new GuestListStoreError(code, message);
}

function newDeviceListId() {
  const random = globalThis.crypto;
  if (typeof random?.randomUUID === "function") return `device:${random.randomUUID()}`;
  // randomUUID is secure-context-only; getRandomValues also works on HTTP
  // previews. Keep cryptographically random UUIDs and the existing ID format.
  if (typeof random?.getRandomValues !== "function") {
    fail("unavailable", "This browser cannot create a random device list identity.");
  }
  const bytes = random.getRandomValues(new Uint8Array(16));
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, byte => byte.toString(16).padStart(2, "0")).join("");
  return `device:${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

function validateDocument({ name, snapshot } = {}) {
  if (typeof name !== "string" || !name.trim()) {
    fail("invalid", "A device list needs a name.");
  }
  if (!(snapshot instanceof Uint8Array) || snapshot.byteLength === 0) {
    fail("invalid", "A device list needs a document snapshot.");
  }
}

function validateRecord(record) {
  if (!record || typeof record.id !== "string" || !record.id.startsWith("device:")) {
    fail("corrupt", "The saved device list has an invalid identity.");
  }
  if (!Number.isSafeInteger(record.revision) || record.revision < 1) {
    fail("corrupt", "The saved device list has an invalid revision.");
  }
  try {
    validateDocument(record);
  } catch {
    fail("corrupt", "The saved device list is damaged; it has not been removed.");
  }
  return record;
}

function checkRevision(record, expectedRevision) {
  if (!record) fail("missing", "The device list no longer exists.");
  validateRecord(record);
  if (record.revision !== expectedRevision) {
    fail("conflict", "Another window changed this list. Merge it before saving again.");
  }
  if (record.revision === Number.MAX_SAFE_INTEGER) {
    fail("invalid", "The device list revision limit has been reached.");
  }
}

// The callback and all request callbacks must be synchronous. Waiting for
// network or a Promise inside an IDB transaction can make it auto-commit.
function transaction(db, mode, run) {
  return new Promise((resolve, reject) => {
    let result;
    let failure;
    const tx = db.transaction(STORE, mode);
    const guard = (callback) => (...args) => {
      try {
        callback(...args);
      } catch (error) {
        failure = error;
        tx.abort();
      }
    };
    tx.oncomplete = () => resolve(result);
    tx.onabort = () => reject(failure || tx.error || new Error("Device list save aborted."));
    guard(run)(tx.objectStore(STORE), (value) => { result = value; }, guard);
  });
}

export async function openGuestListStore() {
  if (!globalThis.indexedDB) {
    fail("unavailable", "This browser cannot save device lists.");
  }
  const db = await new Promise((resolve, reject) => {
    const request = indexedDB.open(DATABASE, 1);
    let blocked = false;
    request.onupgradeneeded = () => {
      request.result.createObjectStore(STORE, { keyPath: "id" });
    };
    request.onerror = () => reject(request.error);
    request.onblocked = () => {
      blocked = true;
      reject(new GuestListStoreError("blocked", "Close other Ultros windows and retry saving."));
    };
    request.onsuccess = () => {
      if (blocked) request.result.close();
      else resolve(request.result);
    };
  });
  db.onversionchange = () => db.close();

  return {
    close() { db.close(); },

    async create(document) {
      validateDocument(document);
      const record = {
        id: newDeviceListId(),
        revision: 1,
        name: document.name,
        snapshot: document.snapshot.slice(),
      };
      return transaction(db, "readwrite", (store, done) => {
        store.add(record);
        done(record);
      });
    },

    async load(id) {
      return transaction(db, "readonly", (store, done, guard) => {
        store.get(id).onsuccess = guard((event) => {
          const record = event.target.result;
          done(record ? validateRecord(record) : null);
        });
      });
    },

    // Include per-record errors so one damaged document cannot hide healthy
    // lists. Keep damaged records on disk for a future recovery/export UI.
    async list() {
      return transaction(db, "readonly", (store, done) => {
        store.getAll().onsuccess = (event) => done(event.target.result.map((record) => {
          try {
            validateRecord(record);
            return { id: record.id, name: record.name, revision: record.revision };
          } catch (error) {
            return { id: record.id, error: error.code || "corrupt" };
          }
        }));
      });
    },

    async save(id, expectedRevision, document) {
      validateDocument(document);
      // Snapshot the input before the first asynchronous boundary.
      const snapshot = document.snapshot.slice();
      const name = document.name;
      return transaction(db, "readwrite", (store, done, guard) => {
        store.get(id).onsuccess = guard((event) => {
          const previous = event.target.result;
          checkRevision(previous, expectedRevision);
          const record = { id, revision: previous.revision + 1, name, snapshot };
          store.put(record);
          done(record);
        });
      });
    },

    async remove(id, expectedRevision) {
      return transaction(db, "readwrite", (store, done, guard) => {
        store.get(id).onsuccess = guard((event) => {
          checkRevision(event.target.result, expectedRevision);
          store.delete(id);
          done();
        });
      });
    },
  };
}

// Backups deliberately omit storage revision and ID. Restoring creates a new
// device list, never overwrites an existing list or claims an account list.
// Decode, validate the Loro document, then call create to restore.
export function encodeGuestListBackup(document) {
  validateDocument(document);
  let binary = "";
  for (const byte of document.snapshot) binary += String.fromCharCode(byte);
  return JSON.stringify({
    format: BACKUP_FORMAT,
    version: 1,
    name: document.name,
    snapshot: btoa(binary),
  });
}

export function decodeGuestListBackup(text) {
  try {
    const backup = JSON.parse(text);
    if (backup.format !== BACKUP_FORMAT || backup.version !== 1 || typeof backup.snapshot !== "string") {
      fail("invalid", "Unsupported device list backup.");
    }
    const binary = atob(backup.snapshot);
    const document = {
      name: backup.name,
      snapshot: Uint8Array.from(binary, (character) => character.charCodeAt(0)),
    };
    validateDocument(document);
    return document;
  } catch {
    fail("invalid", "This file is not a supported device list backup.");
  }
}

// Runtime exports are bundled into the versioned wasm-bindgen package.


let store;
async function db() { return store ||= openGuestListStore().catch(e => {store = undefined; throw e;}); }
function announce(id) {
  window.dispatchEvent(new CustomEvent('ultros-device-list-change', {detail:id}));
  if (typeof BroadcastChannel !== 'undefined') { const c = new BroadcastChannel('ultros-device-lists'); c.postMessage(id); c.close(); }
}
export async function guestListRecords() { return JSON.stringify(await (await db()).list()); }
export async function guestLoad(id) { return (await db()).load(id); }
export async function guestCreate(name, snapshot) { const r = await (await db()).create({name, snapshot}); announce(r.id); return r; }
export async function guestSave(id, revision, name, snapshot) { const r = await (await db()).save(id, revision, {name, snapshot}); announce(id); return r; }
export async function guestRemove(id, revision) { await (await db()).remove(id, revision); announce(id); }
export async function guestEncode(name, snapshot) { return encodeGuestListBackup({name, snapshot}); }
export async function guestDecode(text) { return decodeGuestListBackup(text); }
export function guestWatch(id, revision, callback) {
  const local = e => { if(e.detail === id) callback(); };
  let checking = false;
  let active = true;
  const focus = async () => {
    if (checking) return;
    checking = true;
    try { const record = await (await db()).load(id); if (active && (!record || record.revision !== revision())) callback(); }
    catch { if (active) callback(); } finally { checking = false; }
  };
  const channel = typeof BroadcastChannel !== 'undefined' ? new BroadcastChannel('ultros-device-lists') : null;
  if(channel) channel.onmessage = e => {if(e.data === id) callback();};
  window.addEventListener('ultros-device-list-change', local);
  window.addEventListener('focus', focus);
  return () => { active = false; channel?.close(); window.removeEventListener('ultros-device-list-change',local); window.removeEventListener('focus',focus); };
}
