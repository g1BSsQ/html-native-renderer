// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
import { readFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import { resolve } from 'node:path';

export function evaluateGate(report, baseline) {
  if (baseline.schema_version !== 1 || !baseline.fixtures ||
      !Array.isArray(report.fixtures)) throw new Error('invalid parity inputs');
  const measured = report.fixtures.filter(f => (f.suite ?? 'screen') === 'screen');
  const byName = new Map();
  for (const f of measured) {
    if (byName.has(f.fixture)) throw new Error('duplicate fixture: ' + f.fixture);
    byName.set(f.fixture, f);
  }
  const expected = new Set(Object.keys(baseline.fixtures));
  const missing = [...expected].filter(name => !byName.has(name));
  const extra = [...byName.keys()].filter(name => !expected.has(name));
  const failures = [];
  if (missing.length) failures.push('missing fixture(s): ' + missing.join(', '));
  if (extra.length) failures.push('extra fixture(s): ' + extra.join(', '));
  for (const [name, b] of Object.entries(baseline.fixtures)) {
    const f = byName.get(name);
    if (!f) continue;
    const iou = f.mean_iou;
    const pixelDiffPct = f.pixel?.changed_pixel_ratio * 100;
    if (![iou, pixelDiffPct, b.mean_iou, b.pixel_diff_pct].every(Number.isFinite)) {
      failures.push(name + ': non-finite metric');
      continue;
    }
    // These limits are inclusive. Ignore sub-ULP subtraction/multiplication
    // noise (e.g. 0.9000 - 0.8980 may round above 0.002 in IEEE 754).
    const roundingEpsilon = 1e-12;
    if (b.mean_iou - iou > 0.002 + roundingEpsilon) {
      failures.push(name + ': mean IoU regression (' + iou + ' vs ' + b.mean_iou + ')');
    }
    if (pixelDiffPct - b.pixel_diff_pct > 0.25 + roundingEpsilon) {
      failures.push(name + ': pixel diff regression (' + pixelDiffPct.toFixed(4) + '% vs ' + b.pixel_diff_pct + '%)');
    }
  }
  return failures;
}

async function main() {
  const [reportFile = 'out/report.json', baselineFile = 'baselines/ci.json'] = process.argv.slice(2);
  const [report, baseline] = await Promise.all([reportFile, baselineFile].map(async path =>
    JSON.parse(await readFile(path, 'utf8'))));
  const failures = evaluateGate(report, baseline);
  if (failures.length) {
    console.error('Parity gate failed:\n' + failures.map(f => '- ' + f).join('\n'));
    process.exitCode = 1;
  } else console.log('Parity gate passed: ' + Object.keys(baseline.fixtures).length + ' screen fixtures (property probes reported separately).');
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  await main();
}
