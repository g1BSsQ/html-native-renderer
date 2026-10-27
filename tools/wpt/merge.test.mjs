// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
import test from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtemp, mkdir, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const commit = 'd5375db28bf2a1dd1ead39ea147d067175e908c9';
const script = fileURLToPath(new URL('./merge.mjs', import.meta.url));

async function runMerge(scenario = 'valid') {
  const root = await mkdtemp(path.join(tmpdir(), 'wpt-merge-unit-'));
  const shardsDir = path.join(root, 'shards');
  const outputDir = path.join(root, 'merged');
  try {
    for (let index = 0; index < 2; index++) {
      if (index === 1 && scenario === 'missing_shard') continue;
      const part = {
        schema_version: 1,
        wpt_commit: commit,
        width: 800,
        auto_height: true,
        per_test_timeout_ms: 20000,
        shard: { index, count: 2 },
        rows: [{
          path: 'css/css-flexbox/' + (index === 0 ? 'a.html' : 'b.html'),
          area: 'css/css-flexbox',
          status: index === 0 ? 'PASS' : 'FAIL',
          properties: []
        }]
      };
      if (index === 1) {
        if (scenario === 'duplicate_row') part.rows[0].path = 'css/css-flexbox/a.html';
        if (scenario === 'wrong_index') part.shard.index = 0;
        if (scenario === 'commit_mismatch') part.wpt_commit = 'f'.repeat(40);
        if (scenario === 'timeout_mismatch') part.per_test_timeout_ms = 19000;
      }
      const folder = path.join(shardsDir, 'wpt-shard-' + index);
      await mkdir(folder, { recursive: true });
      await writeFile(path.join(folder, 'results.json'), JSON.stringify(part) + '\n');
    }
    const outcome = await new Promise((resolve, reject) => {
      const child = spawn(process.execPath, [script], {
        env: {
          ...process.env,
          WPT_SHARD_COUNT: '2',
          WPT_SHARDS_DIR: shardsDir,
          WPT_OUT: outputDir
        }
      });
      let stdout = '', stderr = '';
      child.stdout.on('data', data => { stdout += data; });
      child.stderr.on('data', data => { stderr += data; });
      child.on('error', reject);
      child.on('close', (code, signal) => resolve({ code, signal, stdout, stderr }));
    });
    let merged = null, coverage = null;
    try {
      merged = JSON.parse(await readFile(path.join(outputDir, 'results.json'), 'utf8'));
      coverage = await readFile(path.join(outputDir, 'COVERAGE.md'), 'utf8');
    } catch (error) {
      if (error.code !== 'ENOENT') throw error;
    }
    return { ...outcome, merged, coverage };
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

test('valid shards merge into one complete, sorted result and coverage report', async () => {
  const result = await runMerge();
  assert.equal(result.code, 0, result.stderr);
  assert.equal(result.merged.wpt_commit, commit);
  assert.equal(result.merged.per_test_timeout_ms, 20000);
  assert.deepEqual(result.merged.rows.map(row => row.path), [
    'css/css-flexbox/a.html', 'css/css-flexbox/b.html'
  ]);
  assert.deepEqual(result.merged.rows.map(row => row.status), ['PASS', 'FAIL']);
  assert.match(result.coverage, /css\/css-flexbox/);
});

for (const [scenario, errorPattern] of [
  ['missing_shard', /ENOENT/],
  ['duplicate_row', /duplicate or invalid WPT result/],
  ['wrong_index', /wrong\/missing shard metadata/],
  ['commit_mismatch', /inconsistent WPT metadata across shards/],
  ['timeout_mismatch', /invalid shard results/]
]) {
  test('merge exits non-zero for ' + scenario, async () => {
    const result = await runMerge(scenario);
    assert.notEqual(result.code, 0, 'merge unexpectedly succeeded: ' + scenario);
    assert.match(result.stderr, errorPattern);
    assert.equal(result.merged, null, 'invalid merge wrote results.json');
  });
}
