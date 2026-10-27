// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
import { AREAS } from './lib.mjs';

export function coverageMarkdown(rows, commit) {
  const lines = [
    '# CSS WPT reftest coverage', '',
    'WPT commit: ' + commit + '.', '',
    'Width: 800 CSS px. Auto capture height; layout viewport height 800 CSS px.',
    'Comparison: RGB, exact or declared WPT fuzzy range; no alignment or masking.',
    'Pass rate = PASS / (PASS + FAIL + CRASH + TIMEOUT). EXCLUDED is outside the denominator.',
    'A failure is a pair mismatch, not proof that an individual CSS property is unsupported.',
    '', '| Area | PASS | FAIL | CRASH | TIMEOUT | EXCLUDED | Pass rate |',
    '|---|---:|---:|---:|---:|---:|---:|'
  ];
  for (const area of AREAS) {
    const cases = rows.filter(row => row.area === area);
    const count = status => cases.filter(row => row.status === status).length;
    const measured = cases.filter(row => row.status !== 'EXCLUDED').length;
    const rate = measured ? (100 * count('PASS') / measured).toFixed(1) + '%' : 'N/A';
    lines.push('| ' + area + ' | ' + ['PASS', 'FAIL', 'CRASH', 'TIMEOUT', 'EXCLUDED'].map(count).join(' | ') + ' | ' + rate + ' |');
  }
  const candidates = new Map();
  for (const row of rows) {
    if (!['FAIL', 'CRASH', 'TIMEOUT'].includes(row.status)) continue;
    for (const prop of new Set(row.properties || []))
      candidates.set(prop, (candidates.get(prop) || 0) + 1);
  }
  lines.push('', '## Top properties associated with failing reftests', '',
    'Ranked by failing or non-rendering test files declaring each property.',
    'These associations are not causal evidence; inherited style and reference layout can matter.',
    '', '| Declared property | Affected tests |', '|---|---:|');
  const ranked = [...candidates].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
  for (const [property, count] of ranked.slice(0, 25))
    lines.push('| ' + property.replace(/\|/g, '\\|') + ' | ' + count + ' |');
  if (!ranked.length) lines.push('| None observed | 0 |');
  const exclusions = new Map();
  for (const row of rows.filter(r => r.status === 'EXCLUDED'))
    exclusions.set(row.reason, (exclusions.get(row.reason) || 0) + 1);
  lines.push('', '## Exclusion reasons', '', '| Reason | Tests |', '|---|---:|');
  for (const [reason, count] of [...exclusions].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])))
    lines.push('| ' + reason.replace(/\|/g, '\\|') + ' | ' + count + ' |');
  if (!exclusions.size) lines.push('| None | 0 |');
  lines.push('', 'Full per-test statuses and reasons: results.json.', '');
  return lines.join('\n');
}

