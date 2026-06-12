// Renders icons-src/icon.svg into the PNG set required by the PWA manifest,
// iOS home screen, and the favicon. Run from web/: node icons-src/render-icons.mjs
import { chromium } from '@playwright/test';
import { readFileSync, mkdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
const svg = readFileSync(join(here, 'icon.svg'), 'utf8');
const outDir = join(here, '..', 'public', 'icons');
mkdirSync(outDir, { recursive: true });

// page that shows the SVG at a given size; `pad` (0..1) shrinks the art into a
// safe zone over a solid wood background — used for maskable and apple icons,
// which must be full-bleed squares with no transparency.
function html(size, { pad = 0, bg = 'transparent' } = {}) {
  const inner = Math.round(size * (1 - pad * 2));
  const off = Math.round(size * pad);
  return `<!doctype html><html><body style="margin:0;width:${size}px;height:${size}px;background:${bg};overflow:hidden">
    <div style="position:absolute;left:${off}px;top:${off}px;width:${inner}px;height:${inner}px">${svg.replace(
      '<svg ',
      `<svg width="${inner}" height="${inner}" `,
    )}</div></body></html>`;
}

const targets = [
  { file: 'pwa-192.png', size: 192, opts: {} },
  { file: 'pwa-512.png', size: 512, opts: {} },
  // maskable: full-bleed wood bg, art inside the 80% safe zone
  { file: 'maskable-512.png', size: 512, opts: { pad: 0.1, bg: '#6f4527' } },
  // iOS rounds corners itself — full-bleed, opaque
  { file: 'apple-touch-icon.png', size: 180, opts: { pad: 0.04, bg: '#6f4527' } },
  { file: 'favicon-64.png', size: 64, opts: {} },
];

const browser = await chromium.launch();
const page = await browser.newPage({ deviceScaleFactor: 1 });
for (const { file, size, opts } of targets) {
  await page.setViewportSize({ width: size, height: size });
  await page.setContent(html(size, opts));
  await page.screenshot({
    path: join(outDir, file),
    omitBackground: !opts.bg || opts.bg === 'transparent',
  });
  console.log('rendered', file);
}
await browser.close();
