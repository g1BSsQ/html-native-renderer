// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
import { readFile, writeFile, mkdir, readdir, mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { execFile } from 'node:child_process';
import { PNG } from 'pngjs';
import { AREAS, Excluded, selection, fuzzyRule, comparePixels, localPath, prepareHtml, cssProperties } from './lib.mjs';
import { coverageMarkdown } from './report.mjs';
import { layoutDiff, layoutDiagnostics, capFailureDiagnostics } from './diagnostics.mjs';

const wptRoot = path.resolve(process.env.WPT_ROOT || '.wpt');
const outRoot = path.resolve(process.env.WPT_OUT || 'out/wpt');
const binary = path.resolve(process.env.ISR_BIN || 'target/release/isr');
const TIMEOUT_MS = 20_000;
const SKIP_DIRS = new Set(['reference', 'references', 'support', 'resources', 'images']);

async function* htmlFiles(folder) {
  const entries = await readdir(path.join(wptRoot, folder), { withFileTypes: true });
  for (const entry of entries.sort((a, b) => a.name.localeCompare(b.name))) {
    const relative = path.posix.join(folder, entry.name);
    if (entry.isDirectory() && !SKIP_DIRS.has(entry.name)) yield* htmlFiles(relative);
    else if (entry.isFile() && /\.(?:html?|xht|xhtml)$/i.test(entry.name)) yield relative;
  }
}

function unsupportedArea(area, relative, html) {
  if (area === 'css/css-masking' && !/\bclip-path\b/i.test(relative + '\n' + html))
    return 'masking_outside_clip_path';
  if (area === 'css/css-transforms' &&
      (/3d/i.test(relative) ||
       /\b(?:matrix3d|translate3d|translatez|scale3d|scalez|rotate3d|rotatex|rotatey|perspective|transform-style|backface-visibility)\b/i.test(html)))
    return 'transform_3d';
  return null;
}

async function render(input, png, layout, deadline) {
  const remaining = deadline - Date.now();
  if (remaining <= 0) return { status: 'TIMEOUT', reason: 'per_test_20s_limit' };
  return new Promise((resolve, reject) => {
    execFile(binary, ['render', input, '--width', '800', '--height-auto', '--out', png, '--layout', layout],
      { timeout: remaining, maxBuffer: 1024 * 1024 }, error => {
        if (!error) return resolve(null);
        // A spawn/configuration failure is a broken harness, not a renderer crash.
        // Process exit codes are numeric; spawn errors carry string error codes.
        if (typeof error.code === 'string' && error.code !== 'ETIMEDOUT')
          return reject(error);
        if (error.killed || error.code === 'ETIMEDOUT')
          return resolve({ status: 'TIMEOUT', reason: 'per_test_20s_limit' });
        if (typeof error.code !== 'number' && !error.signal)
          return reject(error);
        return resolve({ status: 'CRASH', reason: 'renderer_error:' +
          String(error.code ?? error.signal ?? error.message).slice(0, 250) });
      });
  });
}

async function crashWithDiagnostics(failure, testLayout, refLayout, reference) {
  const { comparison: _unused, ...details } = capFailureDiagnostics(null,
    await layoutDiagnostics(testLayout, refLayout));
  return { ...failure, reference, ...details };
}

async function execute(relative, area, html, ref, props) {
  const deadline = Date.now() + TIMEOUT_MS;
  const reference = localPath(wptRoot, relative, ref.href);
  if (!reference) throw new Excluded('unsupported_reference_url');
  let referenceHtml;
  try { referenceHtml = await readFile(path.join(wptRoot, reference), 'utf8'); }
  catch (e) {
    if (e.code === 'ENOENT') throw new Excluded('missing_reference:' + reference);
    throw e;
  }
  const refArea = unsupportedArea(area, reference, referenceHtml);
  if (refArea) throw new Excluded('reference_' + refArea);
  const checkedReference = selection('<link rel="match" href="placeholder.html">' + referenceHtml);
  if (checkedReference.reason) throw new Excluded('reference_' + checkedReference.reason);
  const rule = fuzzyRule(html, ref.href);
  const temp = await mkdtemp(path.join(tmpdir(), 'isr-wpt-'));
  try {
    const preparedTest = await prepareHtml(wptRoot, relative, html);
    const preparedRef = await prepareHtml(wptRoot, reference, referenceHtml);
    const testInput = path.join(temp, 'test.html'), refInput = path.join(temp, 'ref.html');
    const testPng = path.join(temp, 'test.png'), refPng = path.join(temp, 'ref.png');
    const testLayout = path.join(temp, 'test.json'), refLayout = path.join(temp, 'ref.json');
    await Promise.all([writeFile(testInput, preparedTest), writeFile(refInput, preparedRef)]);
    const first = await render(testInput, testPng, testLayout, deadline);
    if (first) return first.status === 'CRASH' ?
      crashWithDiagnostics(first, testLayout, refLayout, reference) : { ...first, reference };
    const second = await render(refInput, refPng, refLayout, deadline);
    if (second) return second.status === 'CRASH' ?
      crashWithDiagnostics(second, testLayout, refLayout, reference) : { ...second, reference };
    let actual, expected, blockedTest, blockedRef;
    try {
      [actual, expected, blockedTest, blockedRef] = await Promise.all([
        readFile(testPng).then(data => PNG.sync.read(data)),
        readFile(refPng).then(data => PNG.sync.read(data)),
        readFile(testLayout, 'utf8').then(JSON.parse),
        readFile(refLayout, 'utf8').then(JSON.parse)
      ]);
    } catch (e) {
      return crashWithDiagnostics({
        status: 'CRASH', reason: 'renderer_output_invalid:' + String(e.message).slice(0, 200)
      }, testLayout, refLayout, reference);
    }
    if ((blockedTest.blocked_resources || []).length || (blockedRef.blocked_resources || []).length)
      throw new Excluded('renderer_blocked_resource');
    const result = comparePixels(actual, expected, rule, ref.kind);
    const reason = result.pass ? null :
      !result.dimensions_match ? 'image_dimensions_differ' :
      ref.kind === 'mismatch' ? 'mismatch_reference_matched' : 'pixel_difference_outside_wpt_fuzzy_range';
    const details = result.pass ? { comparison: result } :
      capFailureDiagnostics(result, { layout_diff: layoutDiff(blockedTest, blockedRef) });
    return { status: result.pass ? 'PASS' : 'FAIL', reason, reference,
      properties: props, ...details };
  } finally {
    await rm(temp, { recursive: true, force: true });
  }
}

function shardFor(relative, shardCount) {
  let hash = 2166136261;
  for (let i = 0; i < relative.length; i++)
    hash = Math.imul(hash ^ relative.charCodeAt(i), 16777619) >>> 0;
  return hash % shardCount;
}

async function anyCandidateReftest() {
  // Only scan all areas when a shard would otherwise emit an empty report.
  // An empty WPT selection is legitimate; losing all candidates in a shard is not.
  for (const area of AREAS) {
    for await (const relative of htmlFiles(area)) {
      const html = await readFile(path.join(wptRoot, relative), 'utf8');
      if (selection(html)) return true;
    }
  }
  return false;
}

async function main() {
  const shardCount = Number(process.env.WPT_SHARD_COUNT || '1');
  const shardIndex = Number(process.env.WPT_SHARD_INDEX || '0');
  if (!Number.isInteger(shardCount) || shardCount < 1 || shardCount > 4 ||
      !Number.isInteger(shardIndex) || shardIndex < 0 || shardIndex >= shardCount)
    throw new Error('invalid WPT shard index/count');

  const commit = (await readFile('tools/wpt/wpt-commit.txt', 'utf8')).trim();
  if (!/^[a-f0-9]{40}$/.test(commit)) throw new Error('invalid pinned WPT commit');
  await mkdir(outRoot, { recursive: true });
  const rows = [];
  for (const area of AREAS) {
    for await (const relative of htmlFiles(area)) {
      if (shardFor(relative, shardCount) !== shardIndex) continue;
      const html = await readFile(path.join(wptRoot, relative), 'utf8');
      const picked = selection(html);
      if (!picked) continue;
      const props = cssProperties(html);
      const base = { path: relative, area, properties: props };
      if (picked.reason) {
        rows.push({ ...base, status: 'EXCLUDED', reason: picked.reason });
        continue;
      }
      const reason = unsupportedArea(area, relative, html);
      if (reason) {
        rows.push({ ...base, status: 'EXCLUDED', reason });
        continue;
      }
      try {
        rows.push({ ...base, ...await execute(relative, area, html, picked.refs[0], props) });
      } catch (e) {
        if (e instanceof Excluded) rows.push({ ...base, status: 'EXCLUDED', reason: e.message });
        else throw e;
      }
    }
  }
  if (shardCount > 1 && rows.length === 0 && await anyCandidateReftest())
    throw new Error('WPT shard ' + shardIndex + '/' + shardCount +
      ' selected zero candidate reftests while the full selection is non-empty');
  await writeFile(path.join(outRoot, 'results.json'), JSON.stringify({
    schema_version: 1, wpt_commit: commit, width: 800, auto_height: true,
    per_test_timeout_ms: TIMEOUT_MS, shard: { index: shardIndex, count: shardCount }, rows
  }, null, 2) + '\n');
  await writeFile(path.join(outRoot, 'COVERAGE.md'), coverageMarkdown(rows, commit));
  console.log('WPT candidate reftests: ' + rows.length);
  console.log('Report: ' + path.join(outRoot, 'COVERAGE.md'));
}

main().catch(e => { console.error('WPT harness error:', e); process.exitCode = 1; });
