// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
import test from 'node:test';
import assert from 'node:assert/strict';
import { evaluateGate } from './gate.mjs';

const baseline = { schema_version: 1, fixtures: {
  '01-card': { mean_iou: .9000, edges_le_1px_pct: 80, max_edge_px: 5, pixel_diff_pct: 2.00 },
  '02-list': { mean_iou: .8000, edges_le_1px_pct: 60, max_edge_px: 9, pixel_diff_pct: 4.00 },
}};
const fixture = (name, iou, pct, suite = 'screen') => ({
  fixture: name, suite, mean_iou: iou, pixel: { changed_pixel_ratio: pct / 100 },
});
const report = (...fixtures) => ({ fixtures });

test('pass: unchanged metrics and independent probes', () => {
  assert.deepEqual(evaluateGate(report(
    fixture('01-card', .9000, 2), fixture('02-list', .8000, 4),
    fixture('probes/line-height', .4, 50, 'probe')), baseline), []);
});
test('regress: either threshold breach fails', () => {
  const errors = evaluateGate(report(fixture('01-card', .8979, 2.0),
    fixture('02-list', .8000, 4.251)), baseline);
  assert.equal(errors.length, 2);
  assert.match(errors[0], /IoU regression/);
  assert.match(errors[1], /pixel diff regression/);
});
test('improve: no penalty for improving metrics', () => {
  assert.deepEqual(evaluateGate(report(fixture('01-card', .95, 1.5),
    fixture('02-list', .93, 1.0)), baseline), []);
});
test('missing fixture is an error', () => {
  assert.match(evaluateGate(report(fixture('01-card', .9, 2)), baseline).join('\n'), /missing fixture/);
});
test('extra fixture is an error', () => {
  assert.match(evaluateGate(report(fixture('01-card', .9, 2),
    fixture('02-list', .8, 4), fixture('unexpected', 1, 0)), baseline).join('\n'), /extra fixture/);
});
