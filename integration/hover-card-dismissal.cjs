// Shared by the virtual-grid browser probe; no market data is required.
const assert = require('node:assert/strict');
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

module.exports = async function checkHoverCardDismissal(page, selector = '[data-grid-tooltip]') {
  const viewport = page.viewport();
  const anchors = [];
  for (const anchor of await page.$$(selector)) {
    const box = await anchor.boundingBox();
    if (box && box.x >= 0 && box.y >= 0 &&
        box.x + box.width < viewport.width && box.y + box.height < viewport.height) {
      anchors.push(anchor);
    }
    if (anchors.length === 12) break;
  }
  assert.equal(anchors.length, 12, 'twelve tooltip anchors are visible without scrolling');
  const moveTo = async element => {
    const box = await element.boundingBox();
    assert(box, 'hover target is still mounted');
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  };
  const moveAway = () => page.mouse.move(2, 2);
  const waitClosed = () => page.waitForFunction(
    () => document.querySelectorAll('[role="tooltip"]').length === 0,
    {timeout: 2000},
  );
  await moveAway();
  await waitClosed();
  const initialScroll = await page.evaluate(() => {
    const grid = document.querySelector('.virtual-grid');
    return [scrollX, scrollY, grid?.scrollLeft, grid?.scrollTop];
  });

  // Enter each portal, then leave it. Replacing the hovered DOM node used
  // to lose mouseleave and leave a card behind for every visited cell.
  for (const anchor of anchors) {
    await moveTo(anchor);
    const tooltip = await page.waitForSelector('[role="tooltip"]', {visible: true});
    await moveTo(tooltip);
    await sleep(350);
    assert(await tooltip.evaluate(el => el.isConnected), 'entering a tooltip preserves its DOM node');
    assert.equal(await page.$$eval('[role="tooltip"]', els => els.length), 1, 'only the current card is open');
    await moveAway();
    await waitClosed();
  }

  // The brief close delay still lets a reader return through the gap.
  await moveTo(anchors[0]);
  const retained = await page.waitForSelector('[role="tooltip"]', {visible: true});
  await moveAway();
  await sleep(50);
  await moveTo(anchors[0]);
  await sleep(350);
  assert(await retained.evaluate(el => el.isConnected), 're-entering cancels dismissal without replacing the card');
  await moveAway();
  await waitClosed();

  // Keyboard focus keeps help available when the pointer leaves. Escape
  // dismisses it without moving focus or allowing a delayed open to return.
  await anchors[0].evaluate(el => el.parentElement.focus());
  const focused = await page.waitForSelector('[role="tooltip"]', {visible: true});
  await moveTo(focused);
  await sleep(350);
  await moveAway();
  await sleep(350);
  assert(await focused.evaluate(el => el.isConnected), 'hovering and leaving focused help preserves its overlay');
  await page.keyboard.press('Escape');
  await waitClosed();
  assert(await anchors[0].evaluate(el => document.activeElement === el.parentElement), 'Escape retains anchor focus');
  await anchors[0].evaluate(el => el.parentElement.blur());

  assert.deepEqual(await page.evaluate(() => {
    const grid = document.querySelector('.virtual-grid');
    return [scrollX, scrollY, grid?.scrollLeft, grid?.scrollTop];
  }), initialScroll, 'all dismissal checks ran without scrolling');
};
