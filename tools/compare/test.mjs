import test from 'node:test';
import assert from 'node:assert/strict';
import { geometry, pixels } from './metrics.mjs';
test('geometry measures overlap and maximum edge error', () => {
  const a = { x: 0, y: 0, width: 10, height: 10 };
  assert.deepEqual(geometry(a, a), { iou: 1, max_edge_error_px: 0 });
  assert.equal(geometry(a, { ...a, x: 5 }).iou, 1 / 3);
  assert.equal(geometry(a, { ...a, x: 5 }).max_edge_error_px, 5);
  assert.equal(geometry({ ...a, width: 0 }, { ...a, width: 0 }).iou, 1);
});
test('pixel difference includes unequal page extents', () => {
  const a = { width: 1, height: 1, data: Buffer.from([0, 0, 0, 255]) };
  const b = { width: 1, height: 2, data: Buffer.from([0, 0, 0, 255, 0, 0, 0, 255]) };
  assert.equal(pixels(a, a).changed_pixel_ratio, 0);
  assert.equal(pixels(a, b).changed_pixel_ratio, 0.5);
});
