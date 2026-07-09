// Browser e2e for the realtime-sync showcase: drives the real wasm app
// in headless Chromium (system binary via playwright-core) against the
// real server. Catches what unit/e2e tests can't: wasm panics, main-
// thread livelocks, and render wedges that only happen in a browser.
//
// Run via `just web-e2e` (orchestrates server + dx serve), or directly:
//   BASE_URL=http://127.0.0.1:8123 node e2e/collab.spec.mjs
//
// Regressions this guards (both shipped, both browser-only):
// - vox-websocket reading `.message` off a plain ws error Event → throw
//   inside a wasm-bindgen import → app wedged when the server was down;
// - presence `states()` pruning expired peers during render → store
//   event → re-render → prune → … main-thread livelock + tab crash.

import { chromium } from 'playwright-core';
import { execSync } from 'node:child_process';

const base = process.env.BASE_URL ?? 'http://127.0.0.1:8123';
const exe = process.env.CHROMIUM ?? execSync('which chromium').toString().trim();

let failures = 0;
const check = (ok, what) => {
  console.log(`${ok ? 'PASS' : 'FAIL'}: ${what}`);
  if (!ok) failures += 1;
};

const browser = await chromium.launch({
  executablePath: exe,
  headless: true,
  args: ['--no-sandbox', '--disable-gpu'],
});
const page = await browser.newPage();
page.setDefaultTimeout(8000);
const pageErrors = [];
page.on('pageerror', (e) => pageErrors.push(e.message));
page.on('console', (m) => {
  if (m.type() === 'error' && m.text().includes('wasm-bindgen')) {
    pageErrors.push(m.text());
  }
});
let crashed = false;
page.on('crash', () => { crashed = true; });

// Load + settle. The app connects, hydrates the store, starts the doc
// sync session.
await page.goto(base, { waitUntil: 'domcontentloaded' });
await page.waitForTimeout(5000);
check(pageErrors.length === 0, `home loads without page errors ${pageErrors[0] ?? ''}`);

// Navigate to the collab page the way a user does. A main-thread
// livelock makes this click time out.
let clicked = true;
try {
  await page.click('text=Collab', { timeout: 6000 });
} catch {
  clicked = false;
}
check(clicked, 'Collab nav link is clickable (main thread responsive)');
await page.waitForTimeout(3000);
check(!crashed, 'tab did not crash after navigation');
check(pageErrors.length === 0, `no page errors on /collab ${pageErrors[0] ?? ''}`);

// The page rendered its chrome.
const h2 = await page.locator('h2').allTextContents();
check(h2.join(' ').includes('Collaborative notes'), `collab header rendered (${h2.join('|')})`);
const badge = (await page.locator('.badge').allTextContents()).join('');
check(badge.length > 0, `sync badge rendered (${badge})`);

// Create a note through the real form → the local replica → the wire.
await page.fill('.example-form input >> nth=0', 'e2e note');
await page.fill('.example-form input >> nth=1', 'playwright');
await page.click('.example-form button[type=submit]');
await page.waitForTimeout(2500);
const notes = (await page.locator('.note-text').allTextContents()).join(' | ');
check(notes.includes('e2e note'), `created note renders (${notes})`);

// Presence: at least this client shows up.
const presence = (await page.locator('.presence-strip').allTextContents()).join(' ');
check(/[1-9]/.test(presence), `presence strip shows a peer (${presence.trim()})`);

// Main thread still healthy at the end.
const t0 = Date.now();
await page.evaluate('1+1');
check(Date.now() - t0 < 1000, 'main thread responsive at end');
check(!crashed, 'tab never crashed');

await browser.close().catch(() => {});
console.log(failures === 0 ? 'ALL PASS' : `${failures} FAILURES`);
process.exit(failures === 0 ? 0 : 1);
