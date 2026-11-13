// The bar for done in ADR-0177: five things a person relies on when an agent
// works the window with its own cursor.
import { expect, test, type Page } from '@playwright/test';
import { cursorBundle } from './cursor-bundle';

const PAGE = `<!doctype html><html><head><style>
  body{margin:0;font:14px system-ui}
  .corner{position:fixed;padding:4px 8px}
</style></head><body>
  <main style="padding:40px">
    <button id="run" type="button">Run</button>
    <label>Search tables <input id="search" /></label>
    <button id="approve" type="button" data-agent-refuse>Approve</button>
  </main>
  <button class="corner" style="left:0;top:0">TL</button>
  <button class="corner" style="right:0;top:0">TR</button>
  <button class="corner" style="left:0;bottom:0">BL</button>
  <button class="corner" style="right:0;bottom:0">BR</button>
</body></html>`;

/** A page with the cursor loaded, recording every click and pointer move. */
async function open(page: Page): Promise<void> {
  await page.setContent(PAGE);
  await page.addScriptTag({ content: await cursorBundle() });
  await page.evaluate(() => {
    const w = window as unknown as Record<string, unknown>;
    const lib = w.AgentCursorLib as { AgentCursor: new (d: Document, o: object) => unknown };
    w.cursor = new lib.AgentCursor(document, {
      tag: 'Agent',
      goTo: (hash: string) => hash === '#query',
    });
    w.clicked = [] as string[];
    w.trustedMoves = 0;
    document.addEventListener('click', (e) => {
      (w.clicked as string[]).push((e.target as HTMLElement).id || (e.target as HTMLElement).innerText);
    });
    // A real pointer moving produces trusted events; the cursor must never.
    for (const type of ['mousemove', 'pointermove', 'mousedown', 'pointerdown']) {
      document.addEventListener(type, (e) => {
        if (e.isTrusted) w.trustedMoves = (w.trustedMoves as number) + 1;
      });
    }
  });
}

function perform(page: Page, step: object): Promise<string> {
  return page.evaluate(
    (s) =>
      (window as unknown as { cursor: { perform(x: object): Promise<string> } }).cursor
        .perform(s)
        .catch((e: Error) => `ERROR: ${e.message}`),
    step,
  );
}

test('point moves the cursor onto the target and outlines it', async ({ page }) => {
  await open(page);
  expect(await perform(page, { action: 'point', target: 'Run' })).toBe('pointed at "Run"');
  const target = await page.locator('#run').boundingBox();
  const cursor = await page.locator('.agent-cursor').boundingBox();
  expect(target && cursor).toBeTruthy();
  // The arrow's tip sits on the target's centre.
  expect(Math.abs(cursor!.x - (target!.x + target!.width / 2))).toBeLessThan(2);
  expect(Math.abs(cursor!.y - (target!.y + target!.height / 2))).toBeLessThan(2);
  await expect(page.locator('.agent-box')).toBeVisible();
  await expect(page.locator('.agent-cursor .tag')).toHaveText('Agent');
});

test('click presses the target without moving the OS pointer', async ({ page }) => {
  await open(page);
  expect(await perform(page, { action: 'click', target: 'Run' })).toBe('clicked "Run"');
  expect(await page.evaluate(() => (window as unknown as { clicked: string[] }).clicked)).toEqual([
    'run',
  ]);
  expect(await page.evaluate(() => (window as unknown as { trustedMoves: number }).trustedMoves)).toBe(0);
  // Typing goes the same way: into the field, by its label.
  expect(await perform(page, { action: 'type', target: 'Search tables', value: 'player' })).toContain(
    'typed 6 characters',
  );
  await expect(page.locator('#search')).toHaveValue('player');
});

test('near every corner the bubble stays inside the window, tail toward the target', async ({
  page,
}) => {
  await open(page);
  for (const corner of ['TL', 'TR', 'BL', 'BR']) {
    await perform(page, {
      action: 'point',
      target: corner,
      message: 'ここを見てください — a message long enough to need some room in the bubble',
    });
    const box = (await page.locator('.agent-bubble').boundingBox())!;
    expect(box.x, corner).toBeGreaterThanOrEqual(8);
    expect(box.y, corner).toBeGreaterThanOrEqual(8);
    expect(box.x + box.width, corner).toBeLessThanOrEqual(800 - 8);
    expect(box.y + box.height, corner).toBeLessThanOrEqual(600 - 8);
    const tail = await page.locator('.agent-bubble').getAttribute('data-tail');
    expect(corner.startsWith('T') ? tail === 'top' || tail === 'left' || tail === 'right' : tail !== 'top', corner).toBe(true);
  }
});

test('an unknown target, or a person\'s button, presses nothing', async ({ page }) => {
  await open(page);
  expect(await perform(page, { action: 'click', target: 'Delete everything' })).toContain(
    'nothing on screen matches',
  );
  expect(await perform(page, { action: 'click', target: 'Approve' })).toContain("a person's button");
  expect(await page.evaluate(() => (window as unknown as { clicked: string[] }).clicked)).toEqual([]);
});

test('a place that is not on screen gives the reason instead of acting', async ({ page }) => {
  await open(page);
  expect(await perform(page, { action: 'go', hash: '#review' })).toContain('there is no tab #review');
  expect(await perform(page, { action: 'go', hash: '#query' })).toBe('switched to #query');
});
