// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
import test from 'node:test';
import assert from 'node:assert/strict';
import { compareResults, deltaMarkdown } from './delta.mjs';

const commit = 'd5375db28bf2a1dd1ead39ea147d067175e908c9';
const prefix = 'css/css-flexbox/';
const fixture = (before, after, wptCommit = commit) => ({
  baseline: { wpt_commit: commit, source_run: 40464290504, tests: before },
  results: { wpt_commit: wptCommit, rows: Object.entries(after).map(([path, status]) =>
    ({ path, area: 'css/css-flexbox', status })) }
});

test('a fixed test is reported with its original status and full path', () => {
  const { baseline, results } = fixture({ [prefix + 'fixed.html']: 'FAIL' },
    { [prefix + 'fixed.html']: 'PASS' });
  const delta = compareResults(results, baseline);
  assert.deepEqual(delta.newly_passing, [{ path: prefix + 'fixed.html',
    area: 'css/css-flexbox', from: 'FAIL', to: 'PASS' }]);
  assert.equal(delta.areas['css/css-flexbox'].newly_passing, 1);
  assert.match(deltaMarkdown(delta), /fixed.html/);
});

test('a broken test includes PASS to TIMEOUT transitions', () => {
  const { baseline, results } = fixture({ [prefix + 'broken.html']: 'PASS' },
    { [prefix + 'broken.html']: 'TIMEOUT' });
  const delta = compareResults(results, baseline);
  assert.deepEqual(delta.newly_failing.map(x => [x.from, x.to]), [['PASS', 'TIMEOUT']]);
});

test('a newly excluded test is separate from failing tests', () => {
  const { baseline, results } = fixture({ [prefix + 'excluded.html']: 'PASS' },
    { [prefix + 'excluded.html']: 'EXCLUDED' });
  const delta = compareResults(results, baseline);
  assert.equal(delta.newly_excluded.length, 1);
  assert.equal(delta.newly_failing.length, 0);
});

test('different pinned WPT revisions skip all comparison', () => {
  const { baseline, results } = fixture({ [prefix + 'a.html']: 'PASS' },
    { [prefix + 'a.html']: 'FAIL' }, 'f'.repeat(40));
  const delta = compareResults(results, baseline);
  assert.equal(delta.skipped, true);
  assert.equal(delta.reason, 'wpt_commit_mismatch');
  assert.equal(delta.newly_failing, undefined);
  assert.match(deltaMarkdown(delta), /DELTA SKIPPED/);
});

test('unchanged tests produce an empty delta', () => {
  const { baseline, results } = fixture({ [prefix + 'ok.html']: 'PASS' },
    { [prefix + 'ok.html']: 'PASS' });
  const delta = compareResults(results, baseline);
  assert.deepEqual(delta.counts, { newly_passing: 0, newly_failing: 0,
    newly_excluded: 0, newly_measured: 0, new_to_baseline: 0,
    missing_from_current: 0 });
  assert.deepEqual(delta.newly_passing, []);
  assert.deepEqual(delta.newly_failing, []);
  assert.deepEqual(delta.newly_excluded, []);
  assert.match(deltaMarkdown(delta), /None\./);
});

test('previously unselected test is not misclassified as a fix', () => {
  const { baseline, results } = fixture({}, { [prefix + 'new.html']: 'PASS' });
  const delta = compareResults(results, baseline);
  assert.equal(delta.counts.new_to_baseline, 1);
  assert.equal(delta.counts.newly_passing, 0);
});

test('pending baseline produces an explicit non-gating skip', () => {
  const { results } = fixture({}, {});
  const delta = compareResults(results, { pending: true });
  assert.equal(delta.reason, 'baseline_pending');
  assert.match(deltaMarkdown(delta), /DELTA SKIPPED/);
});

function section(markdown, heading) {
  const marker = '## ' + heading + ' (';
  const from = markdown.indexOf(marker);
  assert.notEqual(from, -1, 'missing section: ' + heading);
  const bodyStart = markdown.indexOf('\n', from);
  const next = markdown.indexOf('\n## ', bodyStart);
  return markdown.slice(bodyStart, next === -1 ? undefined : next);
}

test('PASS to CRASH counts as broken, including the per-area broken count', () => {
  const testPath = prefix + 'pass-to-crash.html';
  const { baseline, results } = fixture({ [testPath]: 'PASS' }, { [testPath]: 'CRASH' });
  const delta = compareResults(results, baseline);
  assert.deepEqual(delta.newly_failing, [
    { path: testPath, area: 'css/css-flexbox', from: 'PASS', to: 'CRASH' }
  ]);
  assert.equal(delta.counts.newly_failing, 1);
  assert.equal(delta.areas['css/css-flexbox'].newly_failing, 1);
  assert.match(section(deltaMarkdown(delta), 'Newly failing tests'), /pass-to-crash\.html/);
});

test('newly failing and newly excluded Markdown sections contain their own exact paths', () => {
  const broken = prefix + 'a-only-broken.html';
  const excluded = prefix + 'b-only-excluded.html';
  const { baseline, results } = fixture(
    { [broken]: 'PASS', [excluded]: 'TIMEOUT' },
    { [broken]: 'FAIL', [excluded]: 'EXCLUDED' }
  );
  const markdown = deltaMarkdown(compareResults(results, baseline));
  const failingSection = section(markdown, 'Newly failing tests');
  const excludedSection = section(markdown, 'Newly excluded tests');
  assert.ok(failingSection.includes(broken), 'broken path missing from failing section');
  assert.ok(!failingSection.includes(excluded), 'excluded path wrongly in failing section');
  assert.ok(excludedSection.includes(excluded), 'excluded path missing from excluded section');
  assert.ok(!excludedSection.includes(broken), 'broken path wrongly in excluded section');
});

test('baseline paths missing from the current run are explicitly counted and listed', () => {
  const missing = prefix + 'removed-from-selection.html';
  const { baseline, results } = fixture(
    { [prefix + 'retained.html']: 'PASS', [missing]: 'FAIL' },
    { [prefix + 'retained.html']: 'PASS' }
  );
  const delta = compareResults(results, baseline);
  assert.equal(delta.counts.missing_from_current, 1);
  assert.equal(delta.areas['css/css-flexbox'].missing_from_current, 1);
  assert.deepEqual(delta.missing_from_current, [
    { path: missing, area: 'css/css-flexbox', from: 'FAIL' }
  ]);
  assert.ok(section(deltaMarkdown(delta),
    'Previously baselined tests missing from current selection').includes(missing));
});

test('rejects inconsistent area metadata instead of trusting supplied row.area', () => {
  const testPath = prefix + 'wrong-area.html';
  const { baseline, results } = fixture({ [testPath]: 'PASS' }, { [testPath]: 'FAIL' });
  results.rows[0].area = 'css/css-grid';
  assert.throws(() => compareResults(results, baseline), /inconsistent area/);
});

test('rejects duplicate current rows before counting any status changes', () => {
  const testPath = prefix + 'duplicate.html';
  const { baseline, results } = fixture({ [testPath]: 'PASS' }, { [testPath]: 'FAIL' });
  results.rows.push({ ...results.rows[0], status: 'CRASH' });
  assert.throws(() => compareResults(results, baseline), /duplicate results row/);
});

test('EXCLUDED to each measured status is newly measured, not fixed or broken', () => {
  const statuses = ['PASS', 'FAIL', 'CRASH', 'TIMEOUT'];
  const before = {}, after = {};
  for (const status of statuses) {
    const testPath = prefix + 'previously-excluded-' + status.toLowerCase() + '.html';
    before[testPath] = 'EXCLUDED';
    after[testPath] = status;
  }
  const { baseline, results } = fixture(before, after);
  const delta = compareResults(results, baseline);
  assert.equal(delta.skipped, false);
  assert.equal(delta.counts.newly_measured, 4);
  assert.equal(delta.areas['css/css-flexbox'].newly_measured, 4);
  assert.equal(delta.newly_measured.length, 4);
  assert.deepEqual(delta.newly_measured.map(entry => entry.to).sort(), statuses.slice().sort());
  assert.ok(delta.newly_measured.every(entry => entry.from === 'EXCLUDED'));
  assert.equal(delta.counts.newly_passing, 0);
  assert.equal(delta.counts.newly_failing, 0);
  assert.equal(delta.counts.newly_excluded, 0);
  const markdown = deltaMarkdown(delta);
  const measured = section(markdown, 'Newly measured tests');
  for (const testPath of Object.keys(after))
    assert.ok(measured.includes(testPath), 'newly measured path missing: ' + testPath);
  assert.match(markdown, /Newly measured \(EXCLUDED → PASS\/FAIL\/CRASH\/TIMEOUT\): 4/);
});

test('per-area table exposes baseline PASS, FAIL and EXCLUDED counts', () => {
  const p = prefix + 'pass.html', f = prefix + 'fail.html', e = prefix + 'excluded.html';
  const { baseline, results } = fixture(
    { [p]: 'PASS', [f]: 'FAIL', [e]: 'EXCLUDED' },
    { [p]: 'PASS', [f]: 'FAIL', [e]: 'EXCLUDED' }
  );
  const markdown = deltaMarkdown(compareResults(results, baseline));
  assert.match(markdown, /\| Area \| Baseline PASS \| Baseline FAIL \| Baseline EXCLUDED \|/);
  const row = markdown.split('\n').find(line => line.startsWith('| css/css-flexbox |'));
  assert.ok(row);
  assert.deepEqual(row.split('|').slice(2, 5).map(Number), [1, 1, 1]);
});
