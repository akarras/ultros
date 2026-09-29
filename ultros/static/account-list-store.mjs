// Account snapshots are primary offline data. Every cooperating tab takes the
// same per-account lock before reading, merging and writing a snapshot/index.
// Never fall back to an unlocked write when Web Locks is unavailable.
const recovery = new Map();
let serial = 0;
const key = (user, list) => `${user}:${list}`;
export function accountRemember(user, list, snapshot, replacementSource = null, contentReady = null) {
  const record = {
    snapshot: snapshot.slice(),
    contentReady,
    replacementSource: replacementSource?.slice() ?? null,
    token: ++serial,
  };
  recovery.set(key(user, list), record);
  return record.token;
}
export function accountRecovery(user, list) {
  return recovery.get(key(user, list))?.snapshot ?? null;
}
export function accountRecoverySource(user, list) {
  return recovery.get(key(user, list))?.replacementSource ?? null;
}
export function accountRecoveryReady(user, list) {
  return recovery.get(key(user, list))?.contentReady ?? null;
}
export function accountForget(user, list) {
  recovery.delete(key(user, list));
}
export function accountPending(user, list, token) {
  return recovery.get(key(user, list))?.token === token;
}
if (typeof window !== "undefined")
  window.addEventListener("beforeunload", (event) => {
    if (recovery.size) {
      event.preventDefault();
      event.returnValue = "";
    }
  });

export async function accountSaveLocked(user, list, token, save) {
  if (!globalThis.navigator?.locks)
    throw new Error("Storage locking unavailable");
  return navigator.locks.request(`ultros-account-lists:${user}`, async () => {
    const result = await save();
    // An earlier save must not clear a newer unsaved edit's recovery copy.
    if (result === true && recovery.get(key(user, list))?.token === token) {
      recovery.delete(key(user, list));
    }
    return result;
  });
}

export function accountRecoveryBackup(name, snapshot) {
  // The existing device restore flow can recover this snapshot without login.
  let binary = "";
  for (const byte of snapshot) binary += String.fromCharCode(byte);
  return JSON.stringify({
    format: "ultros-device-list",
    version: 1,
    name,
    snapshot: btoa(binary),
  });
}

export function accountDownload(name, snapshot) {
  const text = accountRecoveryBackup(name, snapshot);
  const url = URL.createObjectURL(
    new Blob([text], { type: "application/json" }),
  );
  const link = document.createElement("a");
  link.href = url;
  link.download = "ultros-list-recovery.json";
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
