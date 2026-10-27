import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdir, writeFile, readFile } from 'node:fs/promises';
import { reference } from './index.mjs';
import { PNG } from 'pngjs';
test('document scripts and remote subresources are blocked', async () => {
  await mkdir('out/security-test', { recursive: true });
  await writeFile('out/security-test/input.html', '<!doctype html><div id="safe" style="width:40px;height:1200px;background:#00ff00"></div><script>document.querySelector("div").id="unsafe"</script><img src="https://example.org/image.png">');
  const result = await reference('out/security-test/input.html', {
    out: 'out/security-test/ref.png', layout: 'out/security-test/ref-layout.json' });
  assert.ok(result.elements.some(e => e.id === 'safe'));
  assert.ok(!result.elements.some(e => e.id === 'unsafe'));
  assert.ok(result.blocked_resources.includes('https://example.org/image.png'));
  const screenshot = PNG.sync.read(await readFile('out/security-test/ref.png'));
  assert.equal(screenshot.height, result.height);
  assert.ok(screenshot.height > 800);
  const pixel = (1000 * screenshot.width + 20) * 4;
  assert.deepEqual([...screenshot.data.subarray(pixel, pixel + 4)], [0, 255, 0, 255]);
});
