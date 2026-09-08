const { test } = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');

const css = readFileSync(join(__dirname, '../style/tailwind.css'), 'utf8');
const palettes = [...css.matchAll(/\[data-palette="([a-z-]+)"\][^{]*\{([^}]+)\}/g)]
  .filter(([, , body]) => body.includes('--palette-secondary-dark:'));
const rgb = hex => hex.match(/[\da-f]{2}/gi).map(x => parseInt(x, 16) / 255);
const luminance = color => color.map(x => x <= 0.04045 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4)
  .reduce((sum, x, i) => sum + x * [0.2126, 0.7152, 0.0722][i], 0);
const contrast = (a, b) => (Math.max(luminance(a), luminance(b)) + 0.05) /
  (Math.min(luminance(a), luminance(b)) + 0.05);

test('all twelve palettes keep primary and secondary ink legible on their surfaces', () => {
  assert.equal(palettes.length, 12);
  const failures = [];
  for (const [, name, body] of palettes) {
    const token = key => {
      const match = body.match(new RegExp(`--${key}: (#[\\da-f]{6});`));
      assert.ok(match, `${name}: ${key}`);
      return rgb(match[1]);
    };
    for (const mode of ['dark', 'light']) {
      const page = token(`palette-page-${mode}`);
      // Include the light muted surface and the brightest dark card surface.
      const surface = page.map(x => mode === 'dark' ? x * 0.91 + 0.09 : x * 0.96);
      for (const key of [`color-brand-${mode === 'dark' ? 400 : 600}`, `palette-secondary-${mode}`]) {
        const ratio = contrast(token(key), surface);
        if (ratio < 4.5) failures.push(`${name} ${mode} ${key}: ${ratio.toFixed(2)}:1`);
      }
    }
  }
  assert.deepEqual(failures, []);
});
