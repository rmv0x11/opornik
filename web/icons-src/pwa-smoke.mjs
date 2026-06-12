// PWA smoke test against `vite preview`: manifest reachable, service worker
// takes control, and after going OFFLINE the app still loads and the net
// weights are served from cache. Run from web/: node icons-src/pwa-smoke.mjs
import { chromium } from '@playwright/test';

const BASE = 'http://localhost:4173/opornik/';
const browser = await chromium.launch();
const ctx = await browser.newContext();
const page = await ctx.newPage();

await page.goto(BASE, { waitUntil: 'load' });

const manifestHref = await page.getAttribute('link[rel="manifest"]', 'href');
const manifest = await page.evaluate(async (href) => {
  const r = await fetch(href);
  return r.ok ? r.json() : null;
}, manifestHref);
if (!manifest || manifest.short_name !== 'Опорник') throw new Error('manifest missing/wrong');
console.log('manifest OK:', manifest.name, '| icons:', manifest.icons.length);

// registerType 'prompt' doesn't clientsClaim — the page is controlled only
// from the next navigation. Wait for activation, then reload once.
await page.waitForFunction(async () => (await navigator.serviceWorker?.ready) != null, null, {
  timeout: 15000,
});
await page.reload({ waitUntil: 'load' });
await page.waitForFunction(() => navigator.serviceWorker?.controller != null, null, {
  timeout: 15000,
});
console.log('service worker controls the page');

// give precaching a moment to finish, then cut the network
await page.waitForFunction(
  async () => {
    const keys = await caches.keys();
    return keys.length > 0;
  },
  null,
  { timeout: 15000 },
);
await ctx.setOffline(true);
await page.reload({ waitUntil: 'load' });

const text = await page.textContent('body');
if (!text || text.trim().length < 20) throw new Error('app did not render offline');
console.log('offline reload OK, body text length:', text.trim().length);

const modelOk = await page.evaluate(async () => {
  const r = await fetch('/opornik/nardy-v2-gen3.bin');
  return r.ok && (await r.arrayBuffer()).byteLength > 1_000_000;
});
if (!modelOk) throw new Error('model not served from cache while offline');
console.log('net weights served from cache while offline');

await browser.close();
console.log('\nPWA SMOKE: ALL PASSED');
