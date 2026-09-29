const { test } = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const { runInNewContext } = require('node:vm');

const app = join(__dirname, '../ultros-frontend/ultros-app/src');
const script = readFileSync(join(app, 'theme-bootstrap.js'), 'utf8');
const rust = readFileSync(join(__dirname, '../ultros-frontend/ultros-frontend-core/src/global_state/theme.rs'), 'utf8');

function bootstrap({ storage = {}, cookie = '', blocked = false, dark = false } = {}) {
  const attributes = {};
  const context = {
    document: { cookie, documentElement: { setAttribute: (key, value) => { attributes[key] = value; } } },
    window: { matchMedia: () => ({ matches: dark }) },
  };
  Object.defineProperty(context, 'localStorage', { get() {
    if (blocked) throw new Error('Storage access denied');
    return { getItem: (key) => storage[key] ?? null };
  } });
  runInNewContext(script, context);
  return attributes;
}

test('first paint agrees with Rust for every canonical and legacy palette', () => {
  const paletteImpl = rust.split('impl FromStr for ThemePalette')[1].split('/// Global theme')[0];
  const names = new Map([...rust.matchAll(/ThemePalette::(\w+) => "([a-z-]+)"/g)]
    .map(([, variant, name]) => [variant, name]));
  for (const [, saved, variant] of paletteImpl.matchAll(/"([a-z-]+)" => ThemePalette::(\w+)/g)) {
    for (const value of [saved, saved.toUpperCase()]) {
      const expected = names.get(variant);
      assert.ok(expected, variant);
      assert.equal(bootstrap({ storage: { 'theme.palette': value } })['data-palette'], expected);
      assert.equal(bootstrap({ cookie: `theme_palette=${value}` })['data-palette'], expected);
    }
  }
});

test('new visitors and unknown preferences use dark Ultros', () => {
  const expected = { 'data-theme': 'dark', 'data-palette': 'ultros' };
  assert.deepEqual(bootstrap(), expected);
  for (const value of ['', 'unknown', '__proto__', 'constructor']) {
    assert.deepEqual(bootstrap({ storage: { 'theme.mode': value, 'theme.palette': value } }), expected);
  }
});

test('blocked storage still honors cookies and system appearance', () => {
  assert.deepEqual(bootstrap({ blocked: true, cookie: 'theme_mode=SYSTEM;theme_palette=TeAl', dark: true }),
    { 'data-theme': 'dark', 'data-palette': 'limsa' });
  assert.deepEqual(bootstrap({ blocked: true, cookie: 'theme_mode=SYSTEM; theme_palette=amber', dark: false }),
    { 'data-theme': 'light', 'data-palette': 'uldah' });
  assert.equal(bootstrap({ cookie: 'theme_palette=%E0%A4%A' })['data-palette'], 'ultros');
});

test('storage wins over stale cookies, including empty values', () => {
  assert.deepEqual(bootstrap({ storage: { 'theme.palette': 'sky', 'theme.mode': 'LIGHT' },
    cookie: 'theme_mode=dark; theme_palette=rose' }), { 'data-theme': 'light', 'data-palette': 'ishgard' });
  assert.deepEqual(bootstrap({ storage: { 'theme.palette': '', 'theme.mode': '' },
    cookie: 'theme_mode=light; theme_palette=rose' }), { 'data-theme': 'dark', 'data-palette': 'ultros' });
});

test('SSR includes the tested bootstrap and the canonical default', () => {
  const shell = readFileSync(join(app, 'lib.rs'), 'utf8');
  assert.match(shell, /<script inner_html=include_str!\("theme-bootstrap\.js"\) \/>/);
  assert.match(shell, /<html[^>]*data-palette="ultros"/);
});
