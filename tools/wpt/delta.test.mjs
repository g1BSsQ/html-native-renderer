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
    newly_excluded: 0, new_to_baseline: 0, missing_from_current: 0 });
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
