// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const STATUSES = new Set(['PASS', 'FAIL', 'CRASH', 'TIMEOUT', 'EXCLUDED']);

export function makeBaseline(results, sourceRun) {
  if (!/^[a-f0-9]{40}$/.test(results?.wpt_commit || ''))
    throw new Error('results.json has no valid pinned WPT commit');
  if (!Number.isSafeInteger(sourceRun) || sourceRun <= 0)
    throw new Error('source_run must be a positive integer CI run ID');
  if (!Array.isArray(results.rows)) throw new Error('results.json has no rows');
  const tests = Object.create(null);
  for (const row of results.rows) {
    if (typeof row.path !== 'string' || !row.path.startsWith('css/') ||
        !STATUSES.has(row.status) || Object.hasOwn(tests, row.path))
      throw new Error('invalid or duplicate results row: ' + String(row.path));
    tests[row.path] = row.status;
  }
  return { wpt_commit: results.wpt_commit, source_run: sourceRun,
    tests: Object.fromEntries(Object.entries(tests).sort(([a], [b]) => a.localeCompare(b))) };
}

async function main() {
  const [, , input = 'out/wpt/results.json', runId, output = 'baselines/wpt.json'] = process.argv;
  if (!runId) throw new Error('usage: node tools/wpt/baseline.mjs results.json SOURCE_RUN [output.json]');
  const baseline = makeBaseline(JSON.parse(await readFile(input, 'utf8')), Number(runId));
  await mkdir(path.dirname(output), { recursive: true });
  await writeFile(output, JSON.stringify(baseline) + '\n');
  console.log('Generated baseline for ' + Object.keys(baseline.tests).length +
    ' candidate tests from run ' + baseline.source_run);
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url))
  main().catch(error => { console.error(error); process.exitCode = 1; });
