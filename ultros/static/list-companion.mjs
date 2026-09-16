// A same-origin, interactive shopping companion. The opener owns all list state.
let companion = null;
let callback = null;
let snapshot = null;
let opening = null;
let generation = 0;
let surface = null;

const css = `
:root{color-scheme:dark;font:15px system-ui,sans-serif;background:#111827;color:#f3f4f6}
*{box-sizing:border-box}body{margin:0;padding:16px}h1{font-size:18px;margin:0 0 6px}
p{margin:6px 0;color:#cbd5e1;font-size:13px}header{position:sticky;top:0;background:#111827;padding-bottom:12px}
article{border:1px solid #475569;border-radius:10px;padding:12px;margin:10px 0;background:#1e293b}
article.done{opacity:.65}h2{font-size:15px;margin:0 0 6px;overflow-wrap:anywhere}
button,input{font:inherit;border:1px solid #64748b;border-radius:6px;padding:7px;background:#334155;color:white}
button,input{min-height:44px;min-width:44px}button{cursor:pointer}button:disabled{cursor:default;opacity:.5}button:focus-visible,input:focus-visible{outline:3px solid #a5b4fc;outline-offset:2px}
.actions{display:flex;gap:6px;align-items:center;flex-wrap:wrap;margin-top:10px}input{width:70px}
.primary{background:#4338ca}footer{display:flex;gap:8px;margin-top:14px}#notice{min-height:18px;color:#fde68a}
`;

function parse(value) {
  const result = JSON.parse(value);
  if (!result || !Array.isArray(result.rows)) throw new Error('Invalid shopping companion data.');
  return result;
}

// The owning Leptos view supplies strings in the active UI locale. English
// fallbacks support older snapshots during a rolling client update.
function label(key, fallback) { return snapshot?.labels?.[key] ?? fallback; }

function element(tag, text, className) {
  const node = companion.document.createElement(tag);
  if (text !== undefined) node.textContent = String(text);
  if (className) node.className = className;
  return node;
}

function notice(message) {
  if (companion && !companion.closed) companion.document.getElementById('notice').textContent = message;
}

function dispatch(action, key = '', quantity = 0) {
  if (!callback) return;
  try { callback(action, String(key), quantity); }
  catch { notice(label('updateFailed', 'Could not update your list. Try again in the main window.')); }
}

function button(label, action, disabled = false) {
  const node = element('button', label);
  node.type = 'button';
  node.disabled = disabled;
  const testId = { [snapshot?.labels?.copyName ?? 'Copy name']: 'companion-copy', [snapshot?.labels?.bought ?? 'Bought']: 'companion-buy', [snapshot?.labels?.undo ?? 'Undo']: 'companion-undo', [snapshot?.labels?.nextWorld ?? 'Next world']: 'companion-next' }[label];
  if (testId) node.dataset.testid = testId;
  node.addEventListener('click', action);
  return node;
}

// Preserve real nodes, drafts, focus, scroll and live-region announcements.
function makeSurface() {
  const main = element('main');
  const header = element('header');
  const title = element('h1');
  title.tabIndex = -1;
  const world = element('p');
  const progress = element('p');
  const keepOpen = element('p');
  const mode = element('p');
  mode.id = 'window-mode';
  const status = element('p');
  status.id = 'notice';
  status.setAttribute('role', 'status');
  status.setAttribute('aria-live', 'polite');
  header.append(title, world, progress, keepOpen, mode, status);
  const rows = element('div');
  const empty = element('p');
  const footer = element('footer');
  const undo = button(label('undo', 'Undo'), () => dispatch('undo'));
  const next = button(label('nextWorld', 'Next world'), () => dispatch('next'));
  footer.append(undo, next);
  main.append(header, rows, empty, footer);
  companion.document.body.replaceChildren(main);
  return { main, title, world, progress, keepOpen, mode, rows, empty, undo, next, entries: new Map() };
}

function makeRow(key) {
  const article = element('article');
  article.dataset.key = key;
  const name = element('h2');
  const description = element('p');
  const actions = element('div', undefined, 'actions');
  const entry = { article, name, description, actions, row: null, dirty: false };
  entry.copy = button(label('copyName', 'Copy name'), async () => {
    try {
      await companion.navigator.clipboard.writeText(String(entry.row.name));
      notice(label('copied', 'Item name copied.'));
    } catch { notice(label('clipboardUnavailable', 'Clipboard unavailable. Select and copy the item name above.')); }
  });
  entry.quantity = element('input');
  entry.quantity.type = 'number';
  entry.quantity.min = '1';
  entry.quantity.step = '1';
  entry.quantity.dataset.key = key;
  entry.quantity.addEventListener('input', () => { entry.dirty = true; });
  entry.quantity.addEventListener('keydown', event => {
    if (event.key === 'Escape') {
      event.preventDefault();
      entry.dirty = false;
      entry.quantity.value = String(entry.row.quantity);
    }
  });
  entry.bought = button(label('bought', 'Bought'), () => {
    const amount = Number(entry.quantity.value);
    if (!Number.isSafeInteger(amount) || amount < 1 || amount > Number(entry.row.quantity)) {
      notice(entry.row.invalidQuantity ?? `Enter a whole quantity from 1 to ${entry.row.quantity}.`);
      entry.quantity.focus();
      return;
    }
    entry.dirty = false;
    dispatch('bought', key, amount);
  });
  entry.bought.className = 'primary';
  entry.earlier = element('p');
  actions.append(entry.copy, entry.quantity, entry.bought, entry.earlier);
  article.append(name, description, actions);
  return entry;
}

function usable(node) {
  return node?.isConnected && !node.disabled && !node.closest('[hidden]') && node.getClientRects().length > 0;
}

function render() {
  if (!companion || companion.closed || !snapshot) return;
  const doc = companion.document;
  doc.documentElement.lang = document.documentElement.lang || 'en';
  surface ??= makeSurface();
  const active = doc.activeElement;
  const focusedKey = active?.closest('article')?.dataset.key;
  const focusedIndex = Math.max(0, [...surface.entries.keys()].indexOf(focusedKey));
  const ownedFocus = surface.main.contains(active) && active !== surface.main;
  const scroll = { x: companion.scrollX, y: companion.scrollY };
  doc.title = `${snapshot.title || label('shoppingList', 'Shopping list')} · Ultros`;
  surface.title.textContent = snapshot.title || label('shoppingList', 'Shopping list');
  surface.world.textContent = snapshot.world || label('shoppingCompanion', 'Shopping companion');
  surface.progress.textContent = snapshot.progress || '';
  surface.keepOpen.textContent = label('keepOpen', 'Keep the Ultros tab open. Prices reflect the main list’s last update.');
  surface.mode.textContent = companion.__ultrosMode === 'window'
    ? label('regularWindow', 'Regular window mode. Always-on-top is available in browsers with Document Picture-in-Picture.') : '';
  surface.mode.hidden = companion.__ultrosMode !== 'window';
  const nextEntries = new Map();
  for (const [index, row] of snapshot.rows.entries()) {
    const key = String(row.key);
    const entry = surface.entries.get(key) ?? makeRow(key);
    entry.row = row;
    entry.article.className = row.done ? 'done' : '';
    entry.name.textContent = row.name;
    entry.description.textContent = row.description ?? `${row.quality || label('anyQuality', 'Any quality')} · ${row.quantity} remaining${row.cost == null ? '' : ` · ${row.cost} gil expected`}${row.done ? ' · Done' : ''}`;
    entry.copy.textContent = label('copyName', 'Copy name');
    entry.bought.textContent = label('bought', 'Bought');
    entry.quantity.max = String(row.quantity);
    entry.quantity.setAttribute('aria-label', row.quantityLabel ?? `Quantity purchased: ${row.name}`);
    if (!entry.dirty) entry.quantity.value = String(row.quantity);
    const buying = snapshot.canEdit && !row.done && Number(row.quantity) > 0;
    entry.quantity.hidden = entry.bought.hidden = !buying;
    entry.quantity.disabled = entry.bought.disabled = row.canBuy === false || !buying;
    entry.earlier.hidden = !buying || row.canBuy !== false;
    entry.earlier.textContent = label('earlierStack', 'Record the earlier stack of this item first.');
    // Moving an already ordered node can itself disturb browser focus.
    const atIndex = surface.rows.children[index];
    if (atIndex !== entry.article) surface.rows.insertBefore(entry.article, atIndex ?? null);
    nextEntries.set(key, entry);
  }
  for (const [key, entry] of surface.entries) if (!nextEntries.has(key)) entry.article.remove();
  surface.entries = nextEntries;
  surface.empty.textContent = label('nothingLeft', 'Nothing left to buy at this stop.');
  surface.empty.hidden = snapshot.rows.length > 0;
  surface.undo.textContent = label('undo', 'Undo');
  surface.undo.hidden = !snapshot.canEdit;
  surface.undo.disabled = !snapshot.canUndoPurchase;
  surface.next.textContent = label('nextWorld', 'Next world');
  surface.next.hidden = !snapshot.hasNext;
  if (ownedFocus) {
    const entries = [...nextEntries.values()];
    // Completed stack: Copy beside its name. Removed stack/stop: next row
    // at that position, then preceding row, Next world, or the heading.
    const fallback = nextEntries.get(focusedKey)?.copy
      ?? entries[Math.min(focusedIndex, entries.length - 1)]?.copy
      ?? (usable(surface.next) ? surface.next : surface.title);
    (usable(active) ? active : fallback).focus({ preventScroll: true });
    companion.scrollTo(scroll.x, scroll.y);
  }
}

// Leptos keeps Shop rows alive. Supply a keyboard successor only when a
// control becomes disabled or its row leaves the current world.
export function watchShopFocus(root) {
  let focused = null;
  let rowKey = null;
  let rowIndex = 0;
  const onFocus = event => {
    focused = event.target;
    const row = focused.closest('[data-shop-key]');
    rowKey = row?.dataset.shopKey;
    rowIndex = Math.max(0, [...root.querySelectorAll('[data-shop-key]')].indexOf(row));
  };
  root.addEventListener('focusin', onFocus);
  const observer = new MutationObserver(() => {
    if (!focused || usable(focused) || !root.getClientRects().length) return;
    const active = root.ownerDocument.activeElement;
    if (active !== root.ownerDocument.body && !root.contains(active)) return;
    const rows = [...root.querySelectorAll('[data-shop-key]')];
    const row = rows.find(node => node.dataset.shopKey === rowKey) ?? rows[Math.min(rowIndex, rows.length - 1)];
    const target = [...(row?.querySelectorAll('button') ?? [])].find(usable) ?? root.querySelector('[data-testid="shop-stop-title"]');
    target?.focus({ preventScroll: true });
  });
  observer.observe(root, { subtree: true, childList: true, attributes: true, attributeFilter: ['disabled', 'hidden'] });
  return () => { observer.disconnect(); root.removeEventListener('focusin', onFocus); };
}

/** Call directly from a user click. Returns "pip" or "window". */
export async function openCompanion(initial, onAction) {
  snapshot = parse(initial);
  callback = onAction;
  if (companion && !companion.closed) { render(); companion.focus(); return companion.__ultrosMode; }
  if (opening) return opening;
  const requestedGeneration = generation;
  opening = (async () => {
    let opened;
    let mode = 'pip';
    try {
      if (!window.documentPictureInPicture?.requestWindow) throw new Error('PiP unavailable');
      opened = await window.documentPictureInPicture.requestWindow({ width: 360, height: 480 });
    } catch {
      mode = 'window';
      opened = window.open('', 'ultros-shopping-companion', 'popup,width=360,height=480');
    }
    if (!opened) throw new Error(label('blocked', 'The browser blocked the companion. Allow pop-ups for Ultros and try again.'));
    if (requestedGeneration !== generation) {
      opened.close();
      throw new Error(label('closed', 'The shopping list was closed while opening its companion.'));
    }
    companion = opened;
    surface = null;
    opened.__ultrosMode = mode;
    opened.document.head.replaceChildren();
    const style = element('style', css);
    opened.document.head.append(style);
    opened.document.documentElement.lang = document.documentElement.lang || 'en';
    opened.addEventListener('pagehide', () => {
      if (companion === opened) { companion = null; callback = null; surface = null; }
    }, { once: true });
    render();
    return mode;
  })();
  try { return await opening; } finally { opening = null; }
}

export function updateCompanion(value) { snapshot = parse(value); render(); }

export function closeCompanion() {
  generation += 1;
  const opened = companion;
  companion = null;
  surface = null;
  callback = null;
  snapshot = null;
  if (opened && !opened.closed) opened.close();
}

window.addEventListener('pagehide', closeCompanion);
