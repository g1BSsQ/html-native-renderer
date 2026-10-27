// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
import { readFile, mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { AREAS } from './lib.mjs';

const STATUSES = ['PASS', 'FAIL', 'CRASH', 'TIMEOUT', 'EXCLUDED'];
const FAILING = new Set(['FAIL', 'CRASH', 'TIMEOUT']);
const emptyCounts = () => Object.fromEntries(STATUSES.map(status => [status, 0]));
const areaFor = testPath => AREAS.find(area => testPath.startsWith(area + '/')) ||
  testPath.split('/').slice(0, 2).join('/');
const byPath = (a, b) => a.path.localeCompare(b.path);

export function compareResults(results, baseline) {
  if (baseline?.pending === true)
    return { schema_version: 1, skipped: true, reason: 'baseline_pending',
      notice: 'WPT baseline is pending; a maintainer must regenerate baselines/wpt.json.' };
  if (!/^[a-f0-9]{40}$/.test(baseline?.wpt_commit || '') ||
      !baseline.tests || typeof baseline.tests !== 'object' || Array.isArray(baseline.tests))
    throw new Error('invalid baseline: expected wpt_commit, source_run and tests map');
  if (!/^[a-f0-9]{40}$/.test(results?.wpt_commit || ''))
    throw new Error('invalid results WPT commit');
  if (results.wpt_commit !== baseline.wpt_commit)
    return { schema_version: 1, skipped: true, reason: 'wpt_commit_mismatch',
      baseline_wpt_commit: baseline.wpt_commit, wpt_commit: results.wpt_commit,
      notice: 'WPT commit differs from baseline (' + baseline.wpt_commit +
        ' vs ' + results.wpt_commit + '); delta skipped.' };
  if (!Array.isArray(results.rows)) throw new Error('invalid results rows');
  const areas = Object.create(null);
  const area = name => (areas[name] ||= { baseline: emptyCounts(), current: emptyCounts(),
    newly_passing: 0, newly_failing: 0, newly_excluded: 0,
    new_to_baseline: 0, missing_from_current: 0 });
  const previous = new Map();
  for (const [testPath, status] of Object.entries(baseline.tests)) {
    if (!testPath.startsWith('css/') || !STATUSES.includes(status))
      throw new Error('invalid baseline entry: ' + testPath);
    previous.set(testPath, status);
    area(areaFor(testPath)).baseline[status]++;
  }
  const seen = new Set();
  const newly_passing = [], newly_failing = [], newly_excluded = [];
  const new_to_baseline = [], missing_from_current = [];
  for (const row of results.rows) {
    if (!row || typeof row.path !== 'string' || !row.path.startsWith('css/') ||
        !STATUSES.includes(row.status) || seen.has(row.path))
      throw new Error('invalid or duplicate results row: ' + String(row?.path));
    seen.add(row.path);
    const name = areaFor(row.path);
    if (row.area !== name) throw new Error('inconsistent area for ' + row.path);
    const stats = area(name);
    stats.current[row.status]++;
    const old = previous.get(row.path);
    const change = { path: row.path, area: name, from: old, to: row.status };
    if (old === undefined) {
      stats.new_to_baseline++;
      new_to_baseline.push({ path: row.path, area: name, to: row.status });
    } else if (FAILING.has(old) && row.status === 'PASS') {
      stats.newly_passing++;
      newly_passing.push(change);
    } else if (old === 'PASS' && FAILING.has(row.status)) {
      stats.newly_failing++;
      newly_failing.push(change);
    } else if (old !== 'EXCLUDED' && row.status === 'EXCLUDED') {
      stats.newly_excluded++;
      newly_excluded.push(change);
    }
  }
  for (const [testPath, status] of previous) {
    if (seen.has(testPath)) continue;
    const name = areaFor(testPath);
    area(name).missing_from_current++;
    missing_from_current.push({ path: testPath, area: name, from: status });
  }
  for (const list of [newly_passing, newly_failing, newly_excluded,
    new_to_baseline, missing_from_current]) list.sort(byPath);
  return { schema_version: 1, skipped: false,
    baseline: { wpt_commit: baseline.wpt_commit, source_run: baseline.source_run },
    wpt_commit: results.wpt_commit,
    counts: { newly_passing: newly_passing.length, newly_failing: newly_failing.length,
      newly_excluded: newly_excluded.length, new_to_baseline: new_to_baseline.length,
      missing_from_current: missing_from_current.length },
    areas: Object.fromEntries(Object.entries(areas).sort(([a], [b]) => a.localeCompare(b))),
    newly_passing, newly_failing, newly_excluded, new_to_baseline, missing_from_current };
}

export function deltaMarkdown(delta) {
  if (delta.skipped) return '# WPT delta\n\n> **DELTA SKIPPED:** ' + delta.notice + '\n\n' +
    'No per-test comparison has been made.\n';
  const c = delta.counts;
  const lines = ['# WPT delta', '',
    'Baseline run: ' + delta.baseline.source_run + '. WPT commit: ' + delta.wpt_commit + '.', '',
    '**Reported only — not a pass/fail gate.**',
    'Only comparisons of the same test at the same pinned WPT revision count as regressions or fixes.',
    'New paths outside the baseline are unmeasured changes to scope, not new passes.', '',
    '- Newly passing (FAIL/CRASH/TIMEOUT → PASS): ' + c.newly_passing,
    '- Newly failing (PASS → FAIL/CRASH/TIMEOUT): ' + c.newly_failing,
    '- Newly excluded (measured → EXCLUDED): ' + c.newly_excluded,
    '- No baseline entry (new selection): ' + c.new_to_baseline,
    '- Missing from current selection: ' + c.missing_from_current,
    '', '## Per-area counts', '',
    '| Area | Baseline PASS | Current PASS | Current FAIL | CRASH | TIMEOUT | EXCLUDED | Fixed | Broken | Newly excluded | New paths | Missing |',
    '|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|'];
  for (const [name, a] of Object.entries(delta.areas)) {
    lines.push('| ' + name + ' | ' +
      [a.baseline.PASS, a.current.PASS, a.current.FAIL, a.current.CRASH,
        a.current.TIMEOUT, a.current.EXCLUDED, a.newly_passing, a.newly_failing,
        a.newly_excluded, a.new_to_baseline, a.missing_from_current].join(' | ') + ' |');
  }
  const section = (heading, rows) => {
    lines.push('', '## ' + heading + ' (' + rows.length + ')', '');
    if (!rows.length) { lines.push('None.'); return; }
    for (const row of rows) lines.push('- ' + String.fromCharCode(96) +
      row.path.replace(/`/g, '') + String.fromCharCode(96) +
      (row.from ? ': ' + row.from + ' → ' + row.to : ': ' + row.to));
  };
  section('Newly passing tests', delta.newly_passing);
  section('Newly failing tests', delta.newly_failing);
  section('Newly excluded tests', delta.newly_excluded);
  section('Previously baselined tests missing from current selection',
    delta.missing_from_current.map(row => ({ ...row, to: 'MISSING' })));
  return lines.join('\n') + '\n';
}

async function main() {
  const [, , resultsFile = 'out/wpt/results.json',
    baselineFile = 'baselines/wpt.json', outDir = 'out/wpt'] = process.argv;
  const [results, baseline] = await Promise.all([
    readFile(resultsFile, 'utf8').then(JSON.parse),
    readFile(baselineFile, 'utf8').then(JSON.parse)
  ]);
  const delta = compareResults(results, baseline);
  await mkdir(outDir, { recursive: true });
  await Promise.all([
    writeFile(path.join(outDir, 'delta.json'), JSON.stringify(delta, null, 2) + '\n'),
    writeFile(path.join(outDir, 'DELTA.md'), deltaMarkdown(delta))
  ]);
  if (delta.skipped) console.log('::notice title=WPT delta skipped::' + delta.notice);
  else console.log('WPT delta: ' + JSON.stringify(delta.counts));
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url))
  main().catch(error => { console.error('WPT delta error:', error); process.exitCode = 1; });
