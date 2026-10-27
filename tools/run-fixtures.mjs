// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
import { readdir, mkdir, readFile, writeFile } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import { reference } from './chromium-ref/index.mjs';
const binary = process.env.ISR_BIN || `target/debug/isr${process.platform === 'win32' ? '.exe' : ''}`;
const wsl = process.env.ISR_WSL;
const timings = [];
const inputFiles = [
  ...(await readdir('fixtures')).filter(f => f.endsWith('.html')).sort().map(f => 'fixtures/' + f),
  ...(await readdir('fixtures/probes')).filter(f => f.endsWith('.html')).sort().map(f => 'fixtures/probes/' + f),
];
for (const input of inputFiles) {
  const name = input.slice('fixtures/'.length, -5), dir = 'out/' + name;
  await mkdir(dir, { recursive: true });
  const start = performance.now();
  const args = ['render', input, '--width', '412', '--height-auto',
    '--out', `${dir}/out.png`, '--layout', `${dir}/out-layout.json`];
  const result = spawnSync(wsl ? 'wsl' : binary,
    wsl ? ['-d', wsl, '--', 'env', 'RAYON_NUM_THREADS=4', binary, ...args] : args, { encoding: 'utf8',
      env: { ...process.env, RAYON_NUM_THREADS: '4' } });
  await writeFile(`${dir}/render.log`, ((result.stdout || '') + (result.stderr || ''))
    .replaceAll(/\/home\/[^/\s]+/g, '/toolchain'));
  if (result.status !== 0) throw new Error(`${input}: renderer failed (${result.status}): ${result.error || result.stderr}`);
  const native_ms = performance.now() - start;
  const ref = await reference(input, { width: 412, out: `${dir}/ref.png`, layout: `${dir}/ref-layout.json` });
  const native = JSON.parse(await readFile(`${dir}/out-layout.json`, 'utf8'));
  if (ref.blocked_resources.length || native.blocked_resources.length)
    throw new Error(`${input}: fixture requested a non-data resource`);
  timings.push({ fixture: name, native_ms });
  console.log(`${name}: rendered both engines`);
}
await writeFile('out/timings.json', JSON.stringify(timings, null, 2) + '\n');
await writeFile('out/environment.json', JSON.stringify({
  node: process.version, browser_platform: process.platform,
  native_platform: wsl ? 'linux (WSL)' : process.platform,
  native_binary: binary, width: 412, viewport_height: 800, device_scale_factor: 1,
}, null, 2) + '\n');
