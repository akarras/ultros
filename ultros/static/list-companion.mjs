// A same-origin, interactive shopping companion. The opener owns all list state.
let companion = null;
let callback = null;
let snapshot = null;
let opening = null;
let generation = 0;
let surface = null;
let lastCopied = null;
let themeObserver = null;

// The companion loads the opener's own stylesheet (see adoptTheme), so the
// app's theme tokens and component classes (`card`, `btn-primary`,
// `btn-secondary`, `input`, `clipboard`) apply here as on the list page. This
// sheet only supplies layout, the touch-target floor, and fallbacks for the
// rare case the app stylesheet fails to load. Class rules from the app
// stylesheet outrank these element selectors regardless of sheet order.
const css = `
:root{font:15px system-ui,sans-serif;background:var(--color-background,#111827);color:var(--color-text,#f3f4f6);color-scheme:dark}
:root[data-theme="light"]{color-scheme:light}
*{box-sizing:border-box}body{margin:0;padding:16px;background:inherit;color:inherit}h1{font-size:18px;margin:0 0 6px}
p{margin:6px 0;color:var(--color-text-muted,#cbd5e1);font-size:13px}header{position:sticky;top:0;z-index:1;background:var(--color-background,#111827);padding-bottom:12px}
article{border:1px solid var(--color-outline,#475569);border-radius:12px;padding:12px;margin:10px 0;background:var(--color-background-elevated,#1e293b)}
article.done{opacity:.65}.heading{display:flex;align-items:center;gap:8px;min-width:0}h2{flex:1;min-width:0;font-size:15px;margin:0;overflow-wrap:anywhere}
button,input{font:inherit;border:1px solid var(--color-outline,#64748b);border-radius:8px;padding:7px;background:var(--color-background-elevated,#334155);color:var(--color-text,#fff)}
button,input{min-height:44px;min-width:44px}button{cursor:pointer}button:disabled{cursor:default;opacity:.5}button:focus-visible,input:focus-visible{outline:3px solid var(--brand-ring,#a5b4fc);outline-offset:2px}
.clipboard{display:inline-flex;align-items:center;justify-content:center;flex:none;padding:0;border:0;background:transparent;color:var(--color-text,#f3f4f6);font-size:1.1rem;border-radius:8px}
.actions{display:flex;gap:6px;align-items:center;flex-wrap:wrap;margin-top:10px}input{width:80px}
footer{display:flex;gap:8px;margin-top:14px}#notice{min-height:18px;font-weight:600;color:var(--color-text,#f3f4f6)}
`;

// Bootstrap Icons clipboard2-fill / clipboard2-check-fill, the same glyphs the
// list page's Clipboard control renders (icondata BsClipboard2Fill/CheckFill).
const CLIPBOARD_ICON = '<path d="M9.5 0a.5.5 0 0 1 .5.5.5.5 0 0 0 .5.5.5.5 0 0 1 .5.5V2a.5.5 0 0 1-.5.5h-5A.5.5 0 0 1 5 2v-.5a.5.5 0 0 1 .5-.5.5.5 0 0 0 .5-.5.5.5 0 0 1 .5-.5z"/><path d="M3.5 1h.585A1.5 1.5 0 0 0 4 1.5V2a1.5 1.5 0 0 0 1.5 1.5h5A1.5 1.5 0 0 0 12 2v-.5q-.001-.264-.085-.5h.585A1.5 1.5 0 0 1 14 2.5v12a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 14.5v-12A1.5 1.5 0 0 1 3.5 1"/>';
const CLIPBOARD_CHECK_ICON = '<path d="M10 .5a.5.5 0 0 0-.5-.5h-3a.5.5 0 0 0-.5.5.5.5 0 0 1-.5.5.5.5 0 0 0-.5.5V2a.5.5 0 0 0 .5.5h5A.5.5 0 0 0 11 2v-.5a.5.5 0 0 0-.5-.5.5.5 0 0 1-.5-.5"/><path d="M4.085 1H3.5A1.5 1.5 0 0 0 2 2.5v12A1.5 1.5 0 0 0 3.5 16h9a1.5 1.5 0 0 0 1.5-1.5v-12A1.5 1.5 0 0 0 12.5 1h-.585q.084.236.085.5V2a1.5 1.5 0 0 1-1.5 1.5h-5A1.5 1.5 0 0 1 4 2v-.5q.001-.264.085-.5m6.769 6.854-3 3a.5.5 0 0 1-.708 0l-1.5-1.5a.5.5 0 1 1 .708-.708L7.5 9.793l2.646-2.647a.5.5 0 0 1 .708.708"/>';

const THEME_ATTRIBUTES = ['data-theme', 'data-palette', 'lang'];

// Link the opener's stylesheets into the companion and keep <html>'s theme
// attributes in step, so a theme or palette change on the list page restyles
// the companion live. Both windows are same-origin, so the app sheet loads.
function adoptTheme(opened) {
  const head = opened.document.head;
  for (const node of document.querySelectorAll('link[rel~="stylesheet"], style')) {
    if (node.tagName === 'LINK') {
      const link = opened.document.createElement('link');
      link.rel = 'stylesheet';
      link.href = node.href;
      head.append(link);
    } else {
      head.append(node.cloneNode(true));
    }
  }
  const mirror = () => {
    if (opened.closed) return;
    for (const name of THEME_ATTRIBUTES) {
      const value = document.documentElement.getAttribute(name);
      if (value === null) opened.document.documentElement.removeAttribute(name);
      else opened.document.documentElement.setAttribute(name, value);
    }
  };
  mirror();
  themeObserver?.disconnect();
  themeObserver = new MutationObserver(mirror);
  themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: THEME_ATTRIBUTES });
}

function icon(copied) {
  const svg = companion.document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  svg.setAttribute('viewBox', '0 0 16 16');
  svg.setAttribute('width', '1em');
  svg.setAttribute('height', '1em');
  svg.setAttribute('fill', 'currentColor');
  svg.setAttribute('aria-hidden', 'true');
  svg.innerHTML = copied ? CLIPBOARD_CHECK_ICON : CLIPBOARD_ICON;
  return svg;
}

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

function button(label, action, testId, className) {
  const node = element('button', label, className);
  node.type = 'button';
  node.dataset.testid = testId;
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
  const undo = button(label('undo', 'Undo'), () => dispatch('undo'), 'companion-undo', 'btn-secondary');
  const next = button(label('nextWorld', 'Next world'), () => dispatch('next'), 'companion-next', 'btn-secondary');
  footer.append(undo, next);
  main.append(header, rows, empty, footer);
  companion.document.body.replaceChildren(main);
  return { main, title, world, progress, keepOpen, mode, rows, empty, undo, next, entries: new Map() };
}

function makeRow(key) {
  const article = element('article', undefined, 'card');
  article.dataset.key = key;
  const heading = element('div', undefined, 'heading');
  const name = element('h2');
  const description = element('p');
  const actions = element('div', undefined, 'actions');
  const entry = { article, name, description, actions, row: null, dirty: false };
  // The list page's Clipboard control: icon-only beside the name, checkmark
  // once this name is the most recently copied text.
  entry.copy = button(undefined, async () => {
    const text = String(entry.row.name);
    try {
      await companion.navigator.clipboard.writeText(text);
      lastCopied = text;
      notice(label('copied', 'Item name copied.'));
      render();
    } catch { notice(label('clipboardUnavailable', 'Clipboard unavailable. Select and copy the item name above.')); }
  }, 'companion-copy', 'clipboard');
  entry.copy.replaceChildren(icon(false));
  entry.copied = false;
  heading.append(name, entry.copy);
  entry.quantity = element('input', undefined, 'input');
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
  }, 'companion-buy', 'btn-primary');
  entry.earlier = element('p');
  actions.append(entry.quantity, entry.bought, entry.earlier);
  article.append(heading, description, actions);
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
    entry.article.className = row.done ? 'card done' : 'card';
    entry.name.textContent = row.name;
    entry.description.textContent = row.description ?? `${row.quality || label('anyQuality', 'Any quality')} · ${row.quantity} remaining${row.cost == null ? '' : ` · ${row.cost} gil expected`}${row.done ? ' · Done' : ''}`;
    const copied = lastCopied !== null && String(row.name) === lastCopied;
    entry.copy.setAttribute('aria-label', copied ? label('copied', 'Item name copied.') : label('copyName', 'Copy name'));
    entry.copy.title = label('copyName', 'Copy name');
    if (copied) entry.copy.dataset.copied = 'true';
    else delete entry.copy.dataset.copied;
    // Swap the glyph only on a state change so focus and hit-testing stay put.
    if (entry.copied !== copied) {
      entry.copied = copied;
      entry.copy.replaceChildren(icon(copied));
    }
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
    adoptTheme(opened);
    const style = element('style', css);
    opened.document.head.append(style);
    opened.document.documentElement.lang = document.documentElement.lang || 'en';
    opened.addEventListener('pagehide', () => {
      if (companion === opened) { companion = null; callback = null; surface = null; themeObserver?.disconnect(); themeObserver = null; }
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
  lastCopied = null;
  themeObserver?.disconnect();
  themeObserver = null;
  if (opened && !opened.closed) opened.close();
}

window.addEventListener('pagehide', closeCompanion);
