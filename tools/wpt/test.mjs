// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
import test from 'node:test';
import assert from 'node:assert/strict';
import { selection, fuzzyRule, comparePixels } from './lib.mjs';

const link = '<link rel="match" href="reference.html">';
const a = { width: 2, height: 1, data: Uint8Array.from([0, 0, 0, 255, 10, 10, 10, 255]) };
const identical = { ...a, data: Uint8Array.from(a.data) };
const near = { ...a, data: Uint8Array.from([0, 0, 0, 255, 15, 10, 10, 255]) };
const far = { ...a, data: Uint8Array.from([2, 0, 0, 255, 35, 10, 10, 255]) };
const exact = { maxDifference: [0, 0], totalPixels: [0, 0] };
const fuzzy = { maxDifference: [0, 8], totalPixels: [0, 1] };

test('includes a plain static reftest', () => {
  const result = selection('<!doctype html>' + link + '<style>p{color:green}</style><p>ok</p>');
  assert.equal(result.reason, null);
  assert.deepEqual(result.refs, [{ kind: 'match', href: 'reference.html' }]);
});

test('excludes script even when the document declares a reftest link', () => {
  assert.equal(selection(link + '<script>void 0</script>').reason, 'script_element');
});

test('excludes reftest-wait', () => {
  assert.equal(selection('<html class="reftest-wait">' + link + '</html>').reason, 'reftest_wait');
});

test('excludes external network resources', () => {
  assert.equal(selection(link + '<img src="https://example.invalid/image.png">').reason, 'external_network_resource');
  assert.equal(selection(link + '<style>p{background:url(https://example.invalid/x.png)}</style>').reason,
    'external_network_resource');
});

test('exact comparison passes for equal RGB pixels', () => {
  assert.equal(comparePixels(a, identical, exact).pass, true);
  assert.equal(comparePixels(a, near, exact).pass, false);
});

test('fuzzy comparison accepts differences within both declared ranges', () => {
  const result = comparePixels(a, near, fuzzy);
  assert.equal(result.pass, true);
  assert.equal(result.max_difference, 5);
  assert.equal(result.total_pixels, 1);
});

test('fuzzy comparison rejects differences outside a declared range', () => {
  assert.equal(comparePixels(a, far, fuzzy).pass, false);
  assert.equal(comparePixels(a, near, { maxDifference: [0, 8], totalPixels: [0, 0] }).pass, false);
});

test('fuzzy meta supports inclusive ranges and a reference selector', () => {
  const html = '<meta name="fuzzy" content="maxDifference=0-2;totalPixels=0-3">' +
    '<meta name="fuzzy" content="reference.html:maxDifference=0-8;totalPixels=0-1">';
  assert.deepEqual(fuzzyRule(html, 'reference.html'), fuzzy);
  assert.deepEqual(fuzzyRule(html, 'other.html'), { maxDifference: [0, 2], totalPixels: [0, 3] });
});

test('mismatch reference inverts the comparison outcome', () => {
  assert.equal(comparePixels(a, identical, exact, 'mismatch').pass, false);
  assert.equal(comparePixels(a, far, fuzzy, 'mismatch').pass, true);
});
