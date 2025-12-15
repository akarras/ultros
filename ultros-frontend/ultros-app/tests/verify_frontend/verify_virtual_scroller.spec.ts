import { test, expect } from '@playwright/test';

test('Recipe Analyzer loads and renders', async ({ page }) => {
  await page.goto('/recipe-analyzer');
  // Wait for the page to load (might need to handle login or world selection if applicable,
  // but recipe analyzer usually works standalone or redirects)

  // Assuming defaults or some content loads.
  // We check if the VirtualScroller content is visible.

  // Wait for a row to appear (virtual scroller rows usually have role="row-group" or similar classes we saw in the code)
  await expect(page.locator('[role="rowgroup"]').first()).toBeVisible({ timeout: 15000 });

  await page.screenshot({ path: 'recipe_analyzer.png' });
});

test('Flip Finder Analyzer loads and renders', async ({ page }) => {
  // Flip finder requires a world, e.g., /flip-finder/Ultros
  await page.goto('/flip-finder/Ultros');

  // Wait for results
  await expect(page.locator('[role="rowgroup"]').first()).toBeVisible({ timeout: 15000 });

  await page.screenshot({ path: 'flip_finder.png' });
});
