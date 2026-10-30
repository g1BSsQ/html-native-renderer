// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
import { readFile } from 'node:fs/promises';

const MAX_BANDS = 8;
const MAX_BOXES = 12;
const MAX_DIAGNOSTIC_BYTES = 2047; // Strictly less than 2 KiB.

function colorHistogram(image, box) {
  const counts = new Map();
  for (let y = box.y; y < box.y + box.height; y++) {
    for (let x = box.x; x < box.x + box.width; x++) {
      const offset = (y * image.width + x) * 4;
      const color = '#' + Array.from(image.data.subarray(offset, offset + 4),
        channel => channel.toString(16).padStart(2, '0')).join('');
      counts.set(color, (counts.get(color) || 0) + 1);
    }
  }
  return [...counts].sort(([ca, na], [cb, nb]) => nb - na || (ca < cb ? -1 : ca > cb ? 1 : 0))
    .slice(0, 3).map(([rgba, count]) => ({ rgba, count }));
}

// Called only after the original PASS/FAIL decision; count only pixels above
// the maximum fuzzy channel allowance, without changing the WPT RGB metric.
export function pixelDiagnostics(actual, reference, maxAllowedDifference) {
  const { width, height } = actual;
  const minX = Array(height).fill(width);
  const maxX = Array(height).fill(-1);
  const count = Array(height).fill(0);
  let left = width, right = -1, top = height, bottom = -1;
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const i = (y * width + x) * 4;
      const difference = Math.max(
        Math.abs(actual.data[i] - reference.data[i]),
        Math.abs(actual.data[i + 1] - reference.data[i + 1]),
        Math.abs(actual.data[i + 2] - reference.data[i + 2]));
      if (difference <= maxAllowedDifference) continue;
      left = Math.min(left, x);
      right = Math.max(right, x);
      top = Math.min(top, y);
      bottom = Math.max(bottom, y);
      minX[y] = Math.min(minX[y], x);
      maxX[y] = Math.max(maxX[y], x);
      count[y]++;
    }
  }
  if (right < 0) return { diff_bbox: null, diff_rows: [], test_colors: [], ref_colors: [] };
  const diff_bbox = { x: left, y: top, width: right - left + 1, height: bottom - top + 1 };
  const diff_rows = [];
  const bandHeight = Math.ceil(diff_bbox.height / MAX_BANDS);
  for (let start = top; start <= bottom; start += bandHeight) {
    const end = Math.min(bottom, start + bandHeight - 1);
    let x0 = width, x1 = -1, pixels = 0;
    for (let y = start; y <= end; y++) {
      x0 = Math.min(x0, minX[y]);
      x1 = Math.max(x1, maxX[y]);
      pixels += count[y];
    }
    if (pixels) diff_rows.push({ y: [start, end], x: [x0, x1], pixels });
  }
  return { diff_bbox, diff_rows,
    test_colors: colorHistogram(actual, diff_bbox),
    ref_colors: colorHistogram(reference, diff_bbox) };
}

function geometry(element) {
  if (!element) return null;
  return Object.fromEntries(['x', 'y', 'width', 'height'].map(key => [key, element[key] ?? null]));
}

function short(value, length) {
  return String(value ?? '').slice(0, length);
}

// The renderer emits DOM preorder. Prefer the same tag at the closest
// document-order index; ties go to the earlier reference element.
export function layoutDiff(testLayout, refLayout) {
  const refs = new Map();
  for (const [position, element] of (refLayout?.elements || []).entries()) {
    const key = String(element.tag || '');
    if (!refs.has(key)) refs.set(key, []);
    refs.get(key).push({ element, order: Number.isFinite(element.index) ? element.index : position });
  }
  const output = [];
  for (const [position, element] of (testLayout?.elements || []).entries()) {
    const order = Number.isFinite(element.index) ? element.index : position;
    let closest = null;
    let distance = Infinity;
    for (const candidate of refs.get(String(element.tag || '')) || []) {
      const d = Math.abs(candidate.order - order);
      if (d < distance) { distance = d; closest = candidate.element; }
    }
    const testBox = geometry(element), refBox = geometry(closest);
    if (JSON.stringify(testBox) === JSON.stringify(refBox)) continue;
    const row = { tag: short(element.tag, 32) };
    if (element.id) row.id = short(element.id, 48);
    if (element.class) row.class = short(element.class, 64);
    row.test = testBox;
    row.ref = refBox;
    output.push(row);
    if (output.length === MAX_BOXES) break;
  }
  return output;
}

// Rendering may stop before either JSON file exists. An empty list and an
// explicit unavailability reason distinguish that case from equal geometry.
export async function layoutDiagnostics(testFile, refFile) {
  try {
    const [test, ref] = await Promise.all([
      readFile(testFile, 'utf8').then(JSON.parse),
      readFile(refFile, 'utf8').then(JSON.parse)
    ]);
    if (!Array.isArray(test.elements) || !Array.isArray(ref.elements))
      throw new Error('missing elements');
    return { layout_diff: layoutDiff(test, ref) };
  } catch {
    return { layout_diff: [], layout_diff_unavailable: 'layout_json_missing_or_invalid' };
  }
}

// Cap the combined NEW fields, not the legacy comparison metrics. Remove the
// least informative trailing entries first, consistently for equal inputs.
export function capFailureDiagnostics(comparison, layout) {
  const result = comparison ? { ...comparison } : undefined;
  const extra = { ...layout, layout_diff: [...layout.layout_diff].slice(0, MAX_BOXES) };
  for (const field of ['diff_rows', 'test_colors', 'ref_colors']) {
    if (result?.[field]) result[field] = [...result[field]];
  }
  const added = () => ({
    ...(result && 'diff_bbox' in result ? {
      diff_bbox: result.diff_bbox, diff_rows: result.diff_rows,
      test_colors: result.test_colors, ref_colors: result.ref_colors
    } : {}),
    ...extra
  });
  while (Buffer.byteLength(JSON.stringify(added()), 'utf8') > MAX_DIAGNOSTIC_BYTES) {
    if (extra.layout_diff.length) extra.layout_diff.pop();
    else if (result?.diff_rows?.length) result.diff_rows.pop();
    else if (result?.ref_colors?.length) result.ref_colors.pop();
    else if (result?.test_colors?.length) result.test_colors.pop();
    else throw new Error('WPT diagnostics exceed size cap');
  }
  return { comparison: result, ...extra };
}
