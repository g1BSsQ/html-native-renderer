// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import path from 'node:path';
import { AREAS } from './lib.mjs';
import { coverageMarkdown } from './report.mjs';

async function main() {
  const count = Number(process.env.WPT_SHARD_COUNT || '4');
  if (!Number.isInteger(count) || count < 1 || count > 4)
    throw new Error('invalid WPT_SHARD_COUNT');
  const directory = path.resolve(process.env.WPT_SHARDS_DIR || 'out/wpt/shards');
  const output = path.resolve(process.env.WPT_OUT || 'out/wpt');
  const seen = new Set();
  const rows = [];
  let metadata;
  for (let index = 0; index < count; index++) {
    const file = path.join(directory, 'wpt-shard-' + index, 'results.json');
    const part = JSON.parse(await readFile(file, 'utf8'));
    if (part.shard?.index !== index || part.shard?.count !== count)
      throw new Error('wrong/missing shard metadata: ' + file);
    const shared = { schema_version: part.schema_version, wpt_commit: part.wpt_commit,
      width: part.width, auto_height: part.auto_height,
      per_test_timeout_ms: part.per_test_timeout_ms };
    if (part.schema_version !== 1 || part.per_test_timeout_ms !== 20000 ||
        !/^[a-f0-9]{40}$/.test(part.wpt_commit) || !Array.isArray(part.rows))
      throw new Error('invalid shard results: ' + file);
    if (metadata && JSON.stringify(shared) !== JSON.stringify(metadata))
      throw new Error('inconsistent WPT metadata across shards');
    metadata = shared;
    for (const row of part.rows) {
      if (!row || typeof row.path !== 'string' || seen.has(row.path) ||
          !AREAS.includes(row.area) || !row.path.startsWith(row.area + '/'))
        throw new Error('duplicate or invalid WPT result: ' + String(row?.path));
      seen.add(row.path);
      rows.push(row);
    }
  }
  rows.sort((a, b) => a.path.localeCompare(b.path));
  await mkdir(output, { recursive: true });
  await Promise.all([
    writeFile(path.join(output, 'results.json'), JSON.stringify({ ...metadata, rows }, null, 2) + '\n'),
    writeFile(path.join(output, 'COVERAGE.md'), coverageMarkdown(rows, metadata.wpt_commit))
  ]);
  console.log('Merged ' + count + ' WPT shards: ' + rows.length + ' candidate reftests');
}
main().catch(error => { console.error('WPT merge error:', error); process.exitCode = 1; });
