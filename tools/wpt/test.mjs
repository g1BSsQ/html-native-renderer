// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
import test from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { chmod, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { AREAS, selection, fuzzyRule, comparePixels } from './lib.mjs';

const link = '<link rel="match" href="reference.html">';
const a = { width: 2, height: 1, data: Uint8Array.from([0, 0, 0, 255, 10, 10, 10, 255]) };
const identical = { ...a, data: Uint8Array.from(a.data) };
const near = { ...a, data: Uint8Array.from([0, 0, 0, 255, 15, 10, 10, 255]) };
const far = { ...a, data: Uint8Array.from([2, 0, 0, 255, 35, 10, 10, 255]) };
const exact = { maxDifference: [0, 0], totalPixels: [0, 0] };
const fuzzy = { maxDifference: [0, 8], totalPixels: [0, 1] };

test('includes a plain static reftest', () => {
  const result = selection('<!doctype html>' + link + '<style>p{color:green}</style><p>ok</p>');
  assert.equal(result.reason, null);
  assert.deepEqual(result.refs, [{ kind: 'match', href: 'reference.html' }]);
});

test('excludes script even when the document declares a reftest link', () => {
  assert.equal(selection(link + '<script>void 0</script>').reason, 'script_element');
});

test('excludes reftest-wait', () => {
  assert.equal(selection('<html class="reftest-wait">' + link + '</html>').reason, 'reftest_wait');
});

test('excludes external network resources', () => {
  assert.equal(selection(link + '<img src="https://example.invalid/image.png">').reason, 'external_network_resource');
  assert.equal(selection(link + '<style>p{background:url(https://example.invalid/x.png)}</style>').reason,
    'external_network_resource');
});

test('exact comparison passes for equal RGB pixels', () => {
  assert.equal(comparePixels(a, identical, exact).pass, true);
  assert.equal(comparePixels(a, near, exact).pass, false);
});

test('fuzzy comparison accepts differences within both declared ranges', () => {
  const result = comparePixels(a, near, fuzzy);
  assert.equal(result.pass, true);
  assert.equal(result.max_difference, 5);
  assert.equal(result.total_pixels, 1);
});

test('fuzzy comparison rejects differences outside a declared range', () => {
  assert.equal(comparePixels(a, far, fuzzy).pass, false);
  assert.equal(comparePixels(a, near, { maxDifference: [0, 8], totalPixels: [0, 0] }).pass, false);
});

test('fuzzy meta supports inclusive ranges and a reference selector', () => {
  const html = '<meta name="fuzzy" content="maxDifference=0-2;totalPixels=0-3">' +
    '<meta name="fuzzy" content="reference.html:maxDifference=0-8;totalPixels=0-1">';
  assert.deepEqual(fuzzyRule(html, 'reference.html'), fuzzy);
  assert.deepEqual(fuzzyRule(html, 'other.html'), { maxDifference: [0, 2], totalPixels: [0, 3] });
});

test('mismatch reference inverts the comparison outcome', () => {
  assert.equal(comparePixels(a, identical, exact, 'mismatch').pass, false);
  assert.equal(comparePixels(a, far, fuzzy, 'mismatch').pass, true);
});


test('WPT fuzzy zero-pixel exception accepts identical images with maxDifference minimum above zero', () => {
  const rule = { maxDifference: [1, 1], totalPixels: [0, 0] };
  assert.equal(comparePixels(a, identical, rule, 'match').pass, true);
  assert.equal(comparePixels(a, identical, rule, 'mismatch').pass, false);
});

test('WPT fuzzy zero-channel exception accepts identical images with totalPixels minimum above zero', () => {
  const rule = { maxDifference: [0, 0], totalPixels: [1, 1] };
  assert.equal(comparePixels(a, identical, rule, 'match').pass, true);
  assert.equal(comparePixels(a, identical, rule, 'mismatch').pass, false);
});

test('identical pixels do not satisfy fuzzy ranges whose two lower bounds are positive', () => {
  const rule = { maxDifference: [1, 2], totalPixels: [1, 2] };
  assert.equal(comparePixels(a, identical, rule, 'match').pass, false);
  assert.equal(comparePixels(a, identical, rule, 'mismatch').pass, true);
});

async function runTinyHarness(scenario) {
  const workspace = await mkdtemp(path.join(tmpdir(), 'isr-wpt-launch-test-'));
  const checkout = path.join(workspace, 'checkout');
  const output = path.join(workspace, 'report');
  const script = fileURLToPath(new URL('./index.mjs', import.meta.url));
  const repositoryRoot = fileURLToPath(new URL('../../', import.meta.url));
  try {
    if (scenario !== 'missing_checkout') {
      for (const area of AREAS) await mkdir(path.join(checkout, area), { recursive: true });
      await writeFile(path.join(checkout, 'css/css-flexbox/test.html'),
        '<!doctype html><link rel="match" href="reference.html"><p>test</p>');
      await writeFile(path.join(checkout, 'css/css-flexbox/reference.html'),
        '<!doctype html><p>test</p>');
    }
    const executable = path.join(workspace, 'renderer-stub');
    if (scenario !== 'missing_executable') {
      await writeFile(executable, '#!/bin/sh\nexit 1\n');
      await chmod(executable, scenario === 'not_executable' ? 0o644 : 0o755);
    }
    const processResult = await new Promise((resolve, reject) => {
      const child = spawn(process.execPath, [script], {
        cwd: repositoryRoot,
        env: { ...process.env, WPT_ROOT: checkout, WPT_OUT: output, ISR_BIN: executable }
      });
      let stderr = '';
      child.stderr.on('data', chunk => { stderr += chunk; });
      child.on('error', reject);
      child.on('close', (code, signal) => resolve({ code, signal, stderr }));
    });
    let results = null;
    try {
      results = JSON.parse(await readFile(path.join(output, 'results.json'), 'utf8'));
    } catch (error) {
      if (error.code !== 'ENOENT') throw error;
    }
    return { ...processResult, results };
  } finally {
    await rm(workspace, { recursive: true, force: true });
  }
}

test('missing renderer executable fails the harness rather than writing a CRASH report', async () => {
  const result = await runTinyHarness('missing_executable');
  assert.notEqual(result.code, 0);
  assert.match(result.stderr, /WPT harness error:.*ENOENT/s);
  assert.equal(result.results, null);
});

test('permission-denied renderer executable fails the harness', async () => {
  const result = await runTinyHarness('not_executable');
  assert.notEqual(result.code, 0);
  assert.match(result.stderr, /WPT harness error:.*EACCES/s);
  assert.equal(result.results, null);
});

test('missing WPT checkout fails the harness', async () => {
  const result = await runTinyHarness('missing_checkout');
  assert.notEqual(result.code, 0);
  assert.match(result.stderr, /WPT harness error:.*ENOENT/s);
  assert.equal(result.results, null);
});

test('renderer exit 1 is recorded as a CRASH row without failing the harness', async () => {
  const result = await runTinyHarness('nonzero_exit');
  assert.equal(result.code, 0, result.stderr);
  assert.equal(result.results.rows.length, 1);
  assert.equal(result.results.rows[0].path, 'css/css-flexbox/test.html');
  assert.equal(result.results.rows[0].status, 'CRASH');
  assert.equal(result.results.rows[0].reason, 'renderer_error:1');
});
