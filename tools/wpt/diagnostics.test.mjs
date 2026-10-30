// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
import test from 'node:test';
import assert from 'node:assert/strict';
import { chmod, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import { performance } from 'node:perf_hooks';
import path from 'node:path';
import { PNG } from 'pngjs';
import { AREAS, comparePixels } from './lib.mjs';
import { layoutDiff, guardedLayoutDiff, layoutDiagnostics, pixelDiagnostics, capFailureDiagnostics } from './diagnostics.mjs';

const exact = { maxDifference: [0, 0], totalPixels: [0, 0] };
const rgba = (r, g = 0, b = 0, a = 255) => [r, g, b, a];
function png(width, height, pixels) {
  const image = new PNG({ width, height });
  for (let i = 0; i < width * height; i++) image.data.set(pixels[i], i * 4);
  return PNG.sync.read(PNG.sync.write(image));
}
const solid = (width, height, color = rgba(0)) =>
  png(width, height, Array.from({ length: width * height }, () => color));

function diagnosticsSize(result) {
  const { diff_bbox, diff_rows, test_colors, ref_colors } = result.comparison || {};
  return Buffer.byteLength(JSON.stringify({
    ...(result.comparison && 'diff_bbox' in result.comparison ?
      { diff_bbox, diff_rows, test_colors, ref_colors } : {}),
    layout_diff: result.layout_diff,
    ...(result.layout_diff_unavailable ? { layout_diff_unavailable: result.layout_diff_unavailable } : {})
  }), 'utf8');
}

test('bbox excludes pixels exactly at the fuzzy maximum (inclusive boundary)', () => {
  const actual = solid(4, 3);
  const expected = solid(4, 3);
  expected.data.set(rgba(5), (0 * 4 + 0) * 4);
  expected.data.set(rgba(6), (2 * 4 + 3) * 4);
  const result = comparePixels(actual, expected,
    { maxDifference: [0, 5], totalPixels: [0, 0] });
  assert.equal(result.pass, false);
  assert.equal(result.total_pixels, 2); // The WPT metric remains unfiltered.
  assert.equal(result.max_difference, 6);
  assert.deepEqual(result.diff_bbox, { x: 3, y: 2, width: 1, height: 1 });
  assert.deepEqual(result.diff_rows, [{ y: [2, 2], x: [3, 3], pixels: 1 }]);
  assert.deepEqual(result.test_colors, [{ rgba: '#000000ff', count: 1 }]);
  assert.deepEqual(result.ref_colors, [{ rgba: '#060000ff', count: 1 }]);
});

test('bbox is null when a failing fuzzy pixel-count limit has no over-allowance pixels', () => {
  const actual = solid(1, 1);
  const expected = png(1, 1, [rgba(5)]);
  const result = comparePixels(actual, expected,
    { maxDifference: [0, 5], totalPixels: [0, 0] });
  assert.equal(result.pass, false);
  assert.equal(result.diff_bbox, null);
  assert.deepEqual(result.diff_rows, []);
  assert.deepEqual(result.test_colors, []);
  assert.deepEqual(result.ref_colors, []);
});

test('up to eight horizontal bands report y/x ranges and affected pixel counts', () => {
  const actual = solid(3, 16), expected = solid(3, 16);
  for (let y = 0; y < 16; y++) {
    if (y === 4 || y === 5) continue;
    expected.data.set(rgba(20), (y * 3 + (y % 3)) * 4);
  }
  const result = comparePixels(actual, expected, exact);
  assert.equal(result.pass, false);
  assert.deepEqual(result.diff_bbox, { x: 0, y: 0, width: 3, height: 16 });
  assert.deepEqual(result.diff_rows, [
    { y: [0, 1], x: [0, 1], pixels: 2 },
    { y: [2, 3], x: [0, 2], pixels: 2 },
    { y: [6, 7], x: [0, 1], pixels: 2 },
    { y: [8, 9], x: [0, 2], pixels: 2 },
    { y: [10, 11], x: [1, 2], pixels: 2 },
    { y: [12, 13], x: [0, 1], pixels: 2 },
    { y: [14, 15], x: [0, 2], pixels: 2 }
  ]);
  assert.ok(result.diff_rows.length <= 8);
});

test('color histograms count every pixel inside bbox, RGBA included', () => {
  const actual = png(3, 2, [rgba(255), rgba(255), rgba(0, 255),
    rgba(255), rgba(0, 0, 255, 64), rgba(0, 255)]);
  const expected = solid(3, 2);
  const result = comparePixels(actual, expected, exact);
  assert.deepEqual(result.diff_bbox, { x: 0, y: 0, width: 3, height: 2 });
  assert.deepEqual(result.test_colors, [
    { rgba: '#ff0000ff', count: 3 },
    { rgba: '#00ff00ff', count: 2 },
    { rgba: '#0000ff40', count: 1 }
  ]);
  assert.deepEqual(result.ref_colors, [{ rgba: '#000000ff', count: 6 }]);
});

test('alpha-only changes PASS without diagnostic fields', () => {
  const actual = png(1, 1, [rgba(3, 4, 5, 0)]);
  const expected = png(1, 1, [rgba(3, 4, 5, 255)]);
  const result = comparePixels(actual, expected, exact);
  assert.equal(result.pass, true);
  for (const field of ['diff_bbox', 'diff_rows', 'test_colors', 'ref_colors'])
    assert.equal(Object.hasOwn(result, field), false);
  assert.equal(Object.hasOwn({ status: 'PASS', comparison: result }, 'layout_diff'), false);
});

test('mismatch FAIL with identical pixels has null bbox; dimension mismatch has no bbox', () => {
  const identical = solid(2, 1);
  const mismatch = comparePixels(identical, identical, exact, 'mismatch');
  assert.equal(mismatch.pass, false);
  assert.equal(mismatch.diff_bbox, null);
  assert.deepEqual(mismatch.diff_rows, []);
  const dimensions = comparePixels(identical, solid(3, 1), exact);
  assert.equal(dimensions.pass, false);
  assert.equal(dimensions.dimensions_match, false);
  assert.equal(Object.hasOwn(dimensions, 'diff_bbox'), false);
});

const element = (index, tag, x, y, width, height, id = '', klass = '') =>
  ({ index, tag, id, class: klass, x, y, width, height });

test('layout diff uses DOM order, tag, and the nearest reference box', () => {
  const testLayout = { elements: [
    element(0, 'body', 0, 0, 10, 10),
    element(1, 'span', 2, 2, 3, 3),
    element(2, 'div', 5, 5, 4, 4, 'target', 'one two'),
    element(3, 'p', 0, 8, 2, 1)
  ] };
  const refLayout = { elements: [
    element(0, 'body', 0, 0, 10, 10),
    element(1, 'div', 5, 3, 4, 4),
    element(2, 'span', 2, 2, 3, 3),
    element(4, 'div', 9, 9, 4, 4),
    element(5, 'p', 0, 8, 2, 1)
  ] };
  assert.deepEqual(layoutDiff(testLayout, refLayout), [{
    tag: 'div', id: 'target', class: 'one two',
    test: { x: 5, y: 5, width: 4, height: 4 },
    ref: { x: 5, y: 3, width: 4, height: 4 }
  }]);
  assert.deepEqual(layoutDiff({ elements: [element(0, 'aside', 1, 2, 3, 4)] }, refLayout), [{
    tag: 'aside', test: { x: 1, y: 2, width: 3, height: 4 }, ref: null
  }]);
});

test('layout JSON fixtures are loaded; missing JSON is explicitly unavailable', async () => {
  const dir = await mkdtemp(path.join(tmpdir(), 'wpt-layout-diff-'));
  try {
    const testFile = path.join(dir, 'test.json'), refFile = path.join(dir, 'ref.json');
    await writeFile(testFile, JSON.stringify({ elements: [element(0, 'div', 1, 0, 8, 6)] }));
    await writeFile(refFile, JSON.stringify({ elements: [element(0, 'div', 2, 0, 8, 6)] }));
    assert.equal((await layoutDiagnostics(testFile, refFile)).layout_diff.length, 1);
    await rm(refFile);
    assert.deepEqual(await layoutDiagnostics(testFile, refFile), {
      layout_diff: [], layout_diff_unavailable: 'layout_json_missing_or_invalid'
    });
  } finally { await rm(dir, { recursive: true, force: true }); }
});

test('2 KB cap measured on compact JSON of added fields is deterministic and preserves first boxes', () => {
  const base = { diff_bbox: { x: 0, y: 0, width: 10, height: 10 },
    diff_rows: Array.from({ length: 8 }, (_, i) => ({ y: [i, i], x: [0, 9], pixels: 10 })),
    test_colors: [{ rgba: '#000000ff', count: 10 }], ref_colors: [{ rgba: '#ffffffff', count: 10 }] };
  const boxes = Array.from({ length: 100 }, (_, i) => ({
    tag: 'div', id: String(i).padStart(2, '0') + 'x'.repeat(48),
    class: 'z'.repeat(64), test: { x: i, y: i, width: 10, height: 10 },
    ref: { x: i, y: i + 1, width: 10, height: 10 }
  }));
  const first = capFailureDiagnostics(base, { layout_diff: boxes });
  const second = capFailureDiagnostics(base, { layout_diff: boxes });
  assert.deepEqual(first, second);
  assert.ok(diagnosticsSize(first) < 2048);
  assert.ok(first.layout_diff.length < 12);
  assert.equal(first.layout_diff[0].id, boxes[0].id);
  assert.equal(boxes.length, 100);
  assert.deepEqual(first.comparison.diff_bbox, base.diff_bbox);
});

test('layout diff selects the first 12 changes in document order', () => {
  const testLayout = { elements: Array.from({ length: 20 }, (_, i) => element(i, 'div', i, 0, 1, 1)) };
  const refLayout = { elements: Array.from({ length: 20 }, (_, i) => element(i, 'div', i, 1, 1, 1)) };
  const boxes = layoutDiff(testLayout, refLayout);
  assert.equal(boxes.length, 12);
  assert.deepEqual(boxes.map(b => b.test.x), Array.from({ length: 12 }, (_, i) => i));
});

test('PASS/FAIL decisions and original RGB metrics match the unmodified formula', () => {
  const sample = solid(2, 2);
  const changed = png(2, 2, [rgba(0), rgba(4), rgba(9), rgba(0, 0, 0, 1)]);
  for (const [left, right] of [[sample, sample], [sample, changed], [sample, solid(1, 1)]]) {
    for (const rule of [exact, { maxDifference: [0, 4], totalPixels: [0, 1] },
      { maxDifference: [1, 9], totalPixels: [1, 2] }]) {
      for (const kind of ['match', 'mismatch']) {
        let maximum = 0, total = 0;
        const sameSize = left.width === right.width && left.height === right.height;
        if (sameSize) {
          for (let i = 0; i < left.data.length; i += 4) {
            const d = Math.max(...[0, 1, 2].map(channel =>
              Math.abs(left.data[i + channel] - right.data[i + channel])));
            if (d > 0) total++;
            maximum = Math.max(maximum, d);
          }
        }
        const within = sameSize && ((total === 0 && rule.totalPixels[0] === 0) ||
          (maximum === 0 && rule.maxDifference[0] === 0) ||
          (maximum >= rule.maxDifference[0] && maximum <= rule.maxDifference[1] &&
            total >= rule.totalPixels[0] && total <= rule.totalPixels[1]));
        const result = comparePixels(left, right, rule, kind);
        assert.equal(result.pass, kind === 'mismatch' ? !within : within);
        assert.equal(result.total_pixels, sameSize ? total : null);
        assert.equal(result.max_difference, sameSize ? maximum : null);
      }
    }
  }
});

async function runReportScenario(testImage, referenceImage, {
  failRender = false,
  testLayout = { elements: [element(0, 'div', 2, 0, 5, 5, 'target')] },
  refLayout = { elements: [element(0, 'div', 1, 0, 5, 5, 'target')] }
} = {}) {
  const workspace = await mkdtemp(path.join(tmpdir(), 'wpt-diagnostics-report-'));
  const checkout = path.join(workspace, 'checkout');
  const output = path.join(workspace, 'out');
  try {
    for (const area of AREAS) await mkdir(path.join(checkout, area), { recursive: true });
    await writeFile(path.join(checkout, 'css/css-flexbox/test.html'),
      '<!doctype html><link rel="match" href="ref.html"><div id="target">test</div>');
    await writeFile(path.join(checkout, 'css/css-flexbox/ref.html'),
      '<!doctype html><div id="target">test</div>');
    const testPng = path.join(workspace, 'test.png');
    const refPng = path.join(workspace, 'ref.png');
    const testJson = path.join(workspace, 'test.json');
    const refJson = path.join(workspace, 'ref.json');
    await Promise.all([
      writeFile(testPng, PNG.sync.write(testImage)),
      writeFile(refPng, PNG.sync.write(referenceImage)),
      writeFile(testJson, JSON.stringify(testLayout)),
      writeFile(refJson, JSON.stringify(refLayout))
    ]);
    const executable = path.join(workspace, 'renderer-stub');
    await writeFile(executable, failRender ? '#!/bin/sh\nexit 1\n' :
      '#!/bin/sh\ncase "$2" in\n' +
      '  */test.html) cp "$TEST_PNG" "$7"; cp "$TEST_JSON" "$9" ;;\n' +
      '  *) cp "$REF_PNG" "$7"; cp "$REF_JSON" "$9" ;;\n' +
      'esac\n');
    await chmod(executable, 0o755);
    const script = fileURLToPath(new URL('./index.mjs', import.meta.url));
    const cwd = fileURLToPath(new URL('../../', import.meta.url));
    const result = await new Promise((resolve, reject) => {
      const child = spawn(process.execPath, [script], { cwd, env: {
        ...process.env, WPT_ROOT: checkout, WPT_OUT: output, ISR_BIN: executable,
        WPT_SHARD_COUNT: '1', WPT_SHARD_INDEX: '0',
        TEST_PNG: testPng, REF_PNG: refPng, TEST_JSON: testJson, REF_JSON: refJson
      } });
      let stderr = '';
      child.stderr.on('data', data => { stderr += data; });
      child.on('error', reject);
      child.on('close', code => resolve({ code, stderr }));
    });
    assert.equal(result.code, 0, result.stderr);
    const { rows } = JSON.parse(await readFile(path.join(output, 'results.json'), 'utf8'));
    assert.equal(rows.length, 1);
    return rows[0];
  } finally {
    await rm(workspace, { recursive: true, force: true });
  }
}

test('results.json adds diagnostics on FAIL, never on PASS, and marks unavailable CRASH layout', async () => {
  const black = solid(2, 2);
  const red = png(2, 2, [rgba(255), rgba(0), rgba(0), rgba(0)]);
  const passed = await runReportScenario(black, black);
  assert.equal(passed.status, 'PASS');
  for (const field of ['diff_bbox', 'diff_rows', 'test_colors', 'ref_colors'])
    assert.equal(Object.hasOwn(passed.comparison, field), false);
  assert.equal(Object.hasOwn(passed, 'layout_diff'), false);

  const failed = await runReportScenario(red, black);
  assert.equal(failed.status, 'FAIL');
  assert.deepEqual(failed.comparison.diff_bbox, { x: 0, y: 0, width: 1, height: 1 });
  assert.equal(failed.layout_diff[0].id, 'target');
  assert.ok(diagnosticsSize(failed) < 2048);

  const crashed = await runReportScenario(black, black, { failRender: true });
  assert.equal(crashed.status, 'CRASH');
  assert.deepEqual(crashed.layout_diff, []);
  assert.equal(crashed.layout_diff_unavailable, 'layout_json_missing_or_invalid');
});


test('top-three RGBA histograms cut five colors and break frequency ties by ascending color', () => {
  const testColors = [
    ...Array(4).fill(rgba(255)), ...Array(3).fill(rgba(0)),
    ...Array(2).fill(rgba(0, 0, 255)), ...Array(2).fill(rgba(0, 255)),
    rgba(255, 255, 255)
  ];
  const refColors = [
    ...Array(4).fill(rgba(10)), ...Array(3).fill(rgba(20)),
    ...Array(2).fill(rgba(30)), ...Array(2).fill(rgba(40)),
    rgba(50)
  ];
  const result = comparePixels(png(12, 1, testColors), png(12, 1, refColors), exact);
  assert.equal(result.pass, false);
  assert.deepEqual(result.test_colors, [
    { rgba: '#ff0000ff', count: 4 },
    { rgba: '#000000ff', count: 3 },
    { rgba: '#0000ffff', count: 2 }
  ]);
  assert.deepEqual(result.ref_colors, [
    { rgba: '#0a0000ff', count: 4 },
    { rgba: '#140000ff', count: 3 },
    { rgba: '#1e0000ff', count: 2 }
  ]);
});

test('2 KB cap is measured on compact JSON of added fields; trims bands then ref colors then test colors', () => {
  const comparison = {
    diff_bbox: { x: 0, y: 0, width: 100, height: 100 },
    diff_rows: Array.from({ length: 8 }, (_, i) => ({ y: [i, i], x: [0, 9], pixels: 10 })),
    ref_colors: Array.from({ length: 3 }, (_, i) => ({ rgba: '#' + String(i).repeat(8), count: 20 })),
    test_colors: Array.from({ length: 3 }, (_, i) => ({ rgba: '#' + String(i + 3).repeat(8), count: 20 }))
  };
  const compactBytes = override => diagnosticsSize({
    comparison: { ...comparison, ...override }, layout_diff: []
  });
  const bandsFirst = capFailureDiagnostics(comparison, { layout_diff: [] },
    compactBytes({ diff_rows: comparison.diff_rows.slice(0, 7) }));
  assert.equal(bandsFirst.comparison.diff_rows.length, 7);
  assert.equal(bandsFirst.comparison.ref_colors.length, 3);

  const refNext = capFailureDiagnostics(comparison, { layout_diff: [] },
    compactBytes({ diff_rows: [], ref_colors: comparison.ref_colors.slice(0, 2) }));
  assert.equal(refNext.comparison.diff_rows.length, 0);
  assert.equal(refNext.comparison.ref_colors.length, 2);
  assert.equal(refNext.comparison.test_colors.length, 3);

  const testLast = capFailureDiagnostics(comparison, { layout_diff: [] },
    compactBytes({ diff_rows: [], ref_colors: [], test_colors: comparison.test_colors.slice(0, 2) }));
  assert.equal(testLast.comparison.diff_rows.length, 0);
  assert.equal(testLast.comparison.ref_colors.length, 0);
  assert.equal(testLast.comparison.test_colors.length, 2);
  for (const capped of [bandsFirst, refNext, testLast])
    assert.ok(diagnosticsSize(capped) < 2048);
});

test('2 KB cap measured on compact JSON of added fields counts multibyte id/class in UTF-8', () => {
  const entries = Array.from({ length: 12 }, (_, i) => ({
    tag: 'div', id: '漢字🧭'.repeat(12) + String(i),
    class: '界🚀'.repeat(32),
    test: { x: i, y: i, width: 50, height: 20 },
    ref: { x: i + 1, y: i, width: 50, height: 20 }
  }));
  const a = capFailureDiagnostics(null, { layout_diff: entries });
  const b = capFailureDiagnostics(null, { layout_diff: entries });
  assert.deepEqual(a, b);
  assert.ok(a.layout_diff.length > 0 && a.layout_diff.length < 12);
  assert.equal(a.layout_diff[0].id, entries[0].id);
  assert.ok(diagnosticsSize(a) <= 2047);
  assert.ok(Buffer.byteLength(JSON.stringify({ layout_diff: entries }), 'utf8') > 2047);
});

test('missing, null, non-array, and null-entry elements return guarded layout unavailability', () => {
  const valid = { elements: [element(0, 'div', 0, 0, 1, 1)] };
  for (const malformed of [
    {}, { elements: null }, { elements: [null] }, { elements: {} }, null
  ]) {
    assert.deepEqual(guardedLayoutDiff(malformed, valid), {
      layout_diff: [], layout_diff_unavailable: 'layout_json_invalid_shape'
    });
    assert.deepEqual(guardedLayoutDiff(valid, malformed), {
      layout_diff: [], layout_diff_unavailable: 'layout_json_invalid_shape'
    });
  }
});

test('stub renderer malformed elements null and [null] keep FAIL rows and results.json', async () => {
  const expected = solid(2, 2);
  const changed = png(2, 2, [rgba(255), rgba(0), rgba(0), rgba(0)]);
  for (const malformed of [{ elements: null }, { elements: [null] },
    { elements: {} }, {}]) {
    const result = await runReportScenario(changed, expected, { testLayout: malformed });
    assert.equal(result.status, 'FAIL');
    assert.equal(result.comparison.pass, false);
    assert.deepEqual(result.layout_diff, []);
    assert.equal(result.layout_diff_unavailable, 'layout_json_invalid_shape');
    assert.ok(diagnosticsSize(result) < 2048);
  }
});

test('800x1200 synthetic full-difference pixel diagnostics reports wall time', t => {
  const width = 800, height = 1200;
  const actual = { width, height, data: new Uint8Array(width * height * 4) };
  const reference = { width, height, data: new Uint8Array(width * height * 4) };
  for (let i = 0; i < reference.data.length; i += 4) reference.data[i] = 255;
  const start = performance.now();
  const result = pixelDiagnostics(actual, reference, 0);
  const elapsed = performance.now() - start;
  t.diagnostic('800x1200 full-difference pixel diagnostics wall time: ' + elapsed.toFixed(1) + ' ms');
  assert.deepEqual(result.diff_bbox, { x: 0, y: 0, width, height });
  assert.equal(result.diff_rows.length, 8);
  assert.deepEqual(result.test_colors, [{ rgba: '#00000000', count: width * height }]);
  assert.deepEqual(result.ref_colors, [{ rgba: '#ff000000', count: width * height }]);
});
