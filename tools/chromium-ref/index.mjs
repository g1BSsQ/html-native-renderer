// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
import { chromium } from 'playwright';
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { resolve, dirname } from 'node:path';
import { pathToFileURL } from 'node:url';

export async function reference(input, { width = 412, out = 'ref.png', layout = 'ref-layout.json' } = {}) {
  const browser = await chromium.launch({ headless: true });
  try {
    const context = await browser.newContext({ javaScriptEnabled: false,
      viewport: { width, height: 800 }, deviceScaleFactor: 1, colorScheme: 'light', serviceWorkers: 'block' });
    const blocked = [];
    await context.route('**/*', route => {
      if (route.request().url().startsWith('data:')) return route.continue();
      blocked.push(route.request().url());
      return route.abort();
    });
    const page = await context.newPage();
    await page.setContent(await readFile(input, 'utf8'), { waitUntil: 'load' });
    // This is privileged measurement code. Document scripts remain disabled.
    await page.evaluate(() => document.fonts.ready);
    const result = await page.evaluate(() => {
      const properties = ['display', 'position', 'font-family', 'font-size', 'line-height',
        'letter-spacing', 'text-overflow', 'overflow', 'border-radius', 'box-shadow',
        'background-image', 'grid-template-columns', 'gap', 'margin-top', 'padding-top'];
      const elements = [...document.querySelectorAll('*')].map((element, index) => {
        const rect = element.getBoundingClientRect();
        const style = getComputedStyle(element);
        return { index, tag: element.localName, id: element.id,
          class: element.getAttribute('class') || '', x: rect.x, y: rect.y,
          width: rect.width, height: rect.height,
          computed: Object.fromEntries(properties.map(p => [p, style.getPropertyValue(p)])) };
      });
      return { schema_version: 1, engine: 'chromium', width: innerWidth,
        height: Math.ceil(Math.max(800, document.documentElement.scrollHeight,
          ...elements.map(e => e.y + e.height))), viewport_height: 800, device_scale_factor: 1, elements };
    });
    if (result.height > 32768) throw new Error('page exceeds 32768 CSS px height limit');
    await mkdir(dirname(resolve(out)), { recursive: true });
    await mkdir(dirname(resolve(layout)), { recursive: true });
    // Capture beyond the viewport without changing layout's 800px viewport height.
    const session = await context.newCDPSession(page);
    const screenshot = await session.send('Page.captureScreenshot', { format: 'png',
      fromSurface: true, captureBeyondViewport: true,
      clip: { x: 0, y: 0, width, height: result.height, scale: 1 } });
    await writeFile(out, Buffer.from(screenshot.data, 'base64'));
    result.blocked_resources = blocked;
    result.browser_version = browser.version();
    await writeFile(layout, JSON.stringify(result, null, 2) + '\n');
    return result;
  } finally { await browser.close(); }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const [input, ...args] = process.argv.slice(2);
  const options = {};
  for (let i = 0; i < args.length; i += 2) {
    const key = args[i].slice(2);
    if (!['width', 'out', 'layout'].includes(key) || !args[i + 1]) throw new Error('invalid arguments');
    options[key] = key === 'width' ? Number(args[i + 1]) : args[i + 1];
  }
  if (!input) throw new Error('usage: node tools/chromium-ref/index.mjs in.html --width 412 --out ref.png --layout ref-layout.json');
  await reference(input, options);
}
