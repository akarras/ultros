// A same-origin, interactive shopping companion. The opener owns all list state.
let companion = null;
let callback = null;
let snapshot = null;
let opening = null;
let generation = 0;

const css = `
:root{color-scheme:dark;font:15px system-ui,sans-serif;background:#111827;color:#f3f4f6}
*{box-sizing:border-box}body{margin:0;padding:16px}h1{font-size:18px;margin:0 0 6px}
p{margin:6px 0;color:#cbd5e1;font-size:13px}header{position:sticky;top:0;background:#111827;padding-bottom:12px}
article{border:1px solid #475569;border-radius:10px;padding:12px;margin:10px 0;background:#1e293b}
article.done{opacity:.65}h2{font-size:15px;margin:0 0 6px;overflow-wrap:anywhere}
button,input{font:inherit;border:1px solid #64748b;border-radius:6px;padding:7px;background:#334155;color:white}
button{cursor:pointer}button:disabled{cursor:default;opacity:.5}button:focus-visible,input:focus-visible{outline:3px solid #a5b4fc;outline-offset:2px}
.actions{display:flex;gap:6px;align-items:center;flex-wrap:wrap;margin-top:10px}input{width:70px}
.primary{background:#4338ca}footer{display:flex;gap:8px;margin-top:14px}#notice{min-height:18px;color:#fde68a}
`;

function parse(value) {
  const result = JSON.parse(value);
  if (!result || !Array.isArray(result.rows)) throw new Error('Invalid shopping companion data.');
  return result;
}

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
  catch { notice('Could not update your list. Try again in the main window.'); }
}

function button(label, action, disabled = false) {
  const node = element('button', label);
  node.type = 'button';
  node.disabled = disabled;
  const testId = { 'Copy name': 'companion-copy', Bought: 'companion-buy', Undo: 'companion-undo', 'Next world': 'companion-next' }[label];
  if (testId) node.dataset.testid = testId;
  node.addEventListener('click', action);
  return node;
}

function render() {
  if (!companion || companion.closed || !snapshot) return;
  const doc = companion.document;
  // Preserve a partially entered purchase quantity across opener updates.
  const active = doc.activeElement;
  const editing = active?.tagName === 'INPUT' ? { key: active.dataset.key, value: active.value } : null;
  doc.title = `${snapshot.title || 'Shopping list'} · Ultros`;
  const main = element('main');
  const header = element('header');
  header.append(element('h1', snapshot.title || 'Shopping list'));
  header.append(element('p', snapshot.world || 'Shopping companion'));
  header.append(element('p', snapshot.progress || ''));
  header.append(element('p', 'Keep the Ultros tab open. Prices reflect the main list’s last update.'));
  const status = element('p');
  status.id = 'notice';
  status.setAttribute('role', 'status');
  header.append(status);
  main.append(header);
  for (const row of snapshot.rows) {
    const article = element('article', undefined, row.done ? 'done' : '');
    article.append(element('h2', row.name));
    article.append(element('p', `${row.quality || 'Any quality'} · ${row.quantity} remaining${row.cost == null ? '' : ` · ${row.cost} gil expected`}${row.done ? ' · Done' : ''}`));
    const actions = element('div', undefined, 'actions');
    actions.append(button('Copy name', async () => {
      try {
        await companion.navigator.clipboard.writeText(String(row.name));
        notice('Item name copied.');
      } catch { notice('Clipboard unavailable. Select and copy the item name above.'); }
    }));
    if (snapshot.canEdit && !row.done && Number(row.quantity) > 0) {
      const quantity = element('input');
      quantity.type = 'number';
      quantity.min = '1';
      quantity.max = String(row.quantity);
      quantity.step = '1';
      quantity.value = String(row.quantity);
      quantity.dataset.key = String(row.key);
      quantity.setAttribute('aria-label', `Quantity purchased: ${row.name}`);
      const bought = button('Bought', () => {
        const amount = Number(quantity.value);
        if (!Number.isSafeInteger(amount) || amount < 1 || amount > Number(row.quantity)) {
          notice(`Enter a whole quantity from 1 to ${row.quantity}.`);
          quantity.focus();
          return;
        }
        dispatch('bought', row.key, amount);
      });
      bought.className = 'primary';
      bought.disabled = row.canBuy === false;
      quantity.disabled = row.canBuy === false;
      actions.append(quantity, bought);
      if (row.canBuy === false) actions.append(element('p', 'Record the earlier stack of this item first.'));
    }
    article.append(actions);
    main.append(article);
  }
  if (!snapshot.rows.length) main.append(element('p', 'Nothing left to buy at this stop.'));
  const footer = element('footer');
  if (snapshot.canEdit) footer.append(button('Undo', () => dispatch('undo')));
  if (snapshot.hasNext) footer.append(button('Next world', () => dispatch('next')));
  main.append(footer);
  doc.body.replaceChildren(main);
  if (editing) {
    const input = [...doc.querySelectorAll('input')].find((node) => node.dataset.key === editing.key);
    if (input) { input.value = editing.value; input.focus(); }
  }
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
    if (!opened) throw new Error('The browser blocked the companion. Allow pop-ups for Ultros and try again.');
    if (requestedGeneration !== generation) {
      opened.close();
      throw new Error('The shopping list was closed while opening its companion.');
    }
    companion = opened;
    opened.__ultrosMode = mode;
    opened.document.head.replaceChildren();
    const style = element('style', css);
    opened.document.head.append(style);
    opened.document.documentElement.lang = document.documentElement.lang || 'en';
    opened.addEventListener('pagehide', () => {
      if (companion === opened) { companion = null; callback = null; }
    }, { once: true });
    render();
    if (mode === 'window') notice('Regular window mode. Always-on-top is available in browsers with Document Picture-in-Picture.');
    return mode;
  })();
  try { return await opening; } finally { opening = null; }
}

export function updateCompanion(value) { snapshot = parse(value); render(); }

export function closeCompanion() {
  generation += 1;
  const opened = companion;
  companion = null;
  callback = null;
  snapshot = null;
  if (opened && !opened.closed) opened.close();
}

window.addEventListener('pagehide', closeCompanion);
