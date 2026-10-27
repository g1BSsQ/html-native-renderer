// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
import test from 'node:test';
import assert from 'node:assert/strict';
import { makeBaseline } from './baseline.mjs';

const commit = 'd5375db28bf2a1dd1ead39ea147d067175e908c9';

test('baseline maps every test path to status and retains WPT commit and source run', () => {
  const baseline = makeBaseline({
    wpt_commit: commit,
    rows: [
      { path: 'css/css-grid/b.html', status: 'EXCLUDED' },
      { path: 'css/css-flexbox/a.html', status: 'PASS' },
      { path: 'css/css-grid/c.html', status: 'CRASH' },
      { path: 'css/css-grid/d.html', status: 'TIMEOUT' },
      { path: 'css/css-grid/e.html', status: 'FAIL' }
    ]
  }, 123456789);
  assert.equal(baseline.wpt_commit, commit);
  assert.equal(baseline.source_run, 123456789);
  assert.deepEqual(baseline.tests, {
    'css/css-flexbox/a.html': 'PASS',
    'css/css-grid/b.html': 'EXCLUDED',
    'css/css-grid/c.html': 'CRASH',
    'css/css-grid/d.html': 'TIMEOUT',
    'css/css-grid/e.html': 'FAIL'
  });
  assert.deepEqual(Object.keys(baseline.tests), [
    'css/css-flexbox/a.html', 'css/css-grid/b.html',
    'css/css-grid/c.html', 'css/css-grid/d.html', 'css/css-grid/e.html'
  ]);
});

test('baseline rejects missing or invalid pinned commit and source run', () => {
  const rows = [{ path: 'css/css-grid/a.html', status: 'PASS' }];
  assert.throws(() => makeBaseline({ wpt_commit: 'bad', rows }, 1), /valid pinned WPT commit/);
  assert.throws(() => makeBaseline({ wpt_commit: commit, rows }, 0), /source_run/);
  assert.throws(() => makeBaseline({ wpt_commit: commit, rows }, NaN), /source_run/);
});

test('baseline rejects duplicate paths and unsupported statuses', () => {
  const a = { path: 'css/css-grid/a.html', status: 'PASS' };
  assert.throws(() => makeBaseline({ wpt_commit: commit, rows: [a, a] }, 1),
    /invalid or duplicate results row/);
  assert.throws(() => makeBaseline({ wpt_commit: commit,
    rows: [{ path: a.path, status: 'UNKNOWN' }] }, 1), /invalid or duplicate results row/);
});
