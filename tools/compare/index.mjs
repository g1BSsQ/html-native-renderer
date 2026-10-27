// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { PNG } from 'pngjs';
import { geometry, pixels } from './metrics.mjs';

const screens = JSON.parse(await readFile('fixtures/manifest.json', 'utf8'));
const isolated = JSON.parse(await readFile('fixtures/probes/manifest.json', 'utf8'));
const fixtures = Object.fromEntries([
  ...Object.entries(screens).map(([name, meta]) => [name, { ...meta, suite: 'screen' }]),
  ...Object.entries(isolated).map(([name, meta]) => ['probes/' + name, { ...meta, suite: 'probe' }]),
]);
const results = [];
const gapFixtures = new Map();
const gapErrors = new Map();
const probeFixtures = new Map();
function geometryCategory(tag) {
  if (['html', 'body'].includes(tag)) return 'document/body bounds';
  if (['a', 'span', 'strong', 'em', 'b', 'i'].includes(tag)) return 'inline element bounds';
  if (['thead', 'tbody', 'tfoot', 'tr'].includes(tag)) return 'table row/group bounds';
  if (['td', 'th'].includes(tag)) return 'table cell bounds';
  if (['circle', 'path', 'rect', 'g', 'polygon'].includes(tag)) return 'SVG descendant bounds';
  if (/^h[1-6]$/.test(tag)) return 'heading bounds';
  return `<${tag}> bounds`;
}
for (const name of Object.keys(fixtures).sort()) {
  const dir = `out/${name}`;
  const [native, ref] = await Promise.all(['out-layout.json', 'ref-layout.json'].map(async f =>
    JSON.parse(await readFile(`${dir}/${f}`, 'utf8'))));
  if (native.width !== ref.width || native.viewport_height !== ref.viewport_height)
    throw new Error(`${name}: incompatible viewports`);
  const [a, b] = await Promise.all(['out.png', 'ref.png'].map(async f => PNG.sync.read(await readFile(`${dir}/${f}`))));
  if (a.width !== native.width || a.height !== native.height || b.width !== ref.width || b.height !== ref.height)
    throw new Error(`${name}: screenshot dimensions disagree with layout JSON`);
  const pixel = pixels(a, b);
  const diffImage = new PNG({ width: pixel.width, height: pixel.height });
  diffImage.data = pixel.diff;
  await writeFile(`${dir}/diff.png`, PNG.sync.write(diffImage));
  const errors = [], elements = [];
  const record = (category, evidence, kind = 'geometry') => {
    errors.push({ category, kind, ...evidence });
    if (fixtures[name].suite === 'probe') return;
    const categories = kind === 'probe' ? probeFixtures : gapFixtures;
    if (!categories.has(category)) categories.set(category, new Set());
    categories.get(category).add(name);
    if (kind === 'geometry') gapErrors.set(category,
      Math.max(gapErrors.get(category) || 0, evidence.max_edge_error_px || 0));
  };
  for (let i = 0; i < Math.max(native.elements.length, ref.elements.length); i++) {
    const n = native.elements[i], r = ref.elements[i];
    if (!n || !r || n.tag !== r.tag || n.id !== r.id || n.class !== r.class) {
      record('document element sequence', { index: i, native: n, chromium: r });
      continue;
    }
    const metric = geometry(n, r);
    const entry = { index: i, tag: r.tag, id: r.id, class: r.class, ...metric,
      native: { x: n.x, y: n.y, width: n.width, height: n.height },
      chromium: { x: r.x, y: r.y, width: r.width, height: r.height } };
    elements.push(entry);
    if (metric.max_edge_error_px > 1) record(geometryCategory(r.tag), entry);
    for (const [property, value] of Object.entries(r.computed || {})) {
      const actual = n.computed?.[property];
      // Serialization differences are evidence, not proof of unsupported CSS.
      if (actual !== value) errors.push({ category: 'computed style serialization differs',
        index: i, id: r.id, property, native: actual, chromium: value,
        interpretation: 'May be serialization only; inspect geometry and images before attributing a bug.' });
    }
  }
  // Associate measured errors with the authored feature probes. Never infer a root cause.
  for (const probe of fixtures[name].probes) {
    const indices = ref.elements.filter(e => e.id === probe.id).map(e => e.index);
    if (!indices.length) throw new Error(`${name}: probe #${probe.id} missing`);
    for (const index of indices) {
      const entry = elements.find(e => e.index === index);
      const r = ref.elements[index];
      let changed = 0, count = 0;
      for (let y = Math.max(0, Math.floor(r.y)); y < Math.min(pixel.height, Math.ceil(r.y + r.height)); y++)
        for (let x = Math.max(0, Math.floor(r.x)); x < Math.min(pixel.width, Math.ceil(r.x + r.width)); x++) {
          count++; if (pixel.diff[(y * pixel.width + x) * 4] > 16) changed++;
        }
      if ((entry?.max_edge_error_px || 0) > 1 || (count && changed / count > 0.01)) {
        record(`probe: ${probe.feature}`, { index, id: probe.id, property_or_element: probe.feature,
          max_edge_error_px: entry?.max_edge_error_px, changed_pixel_ratio_in_reference_box: count ? changed / count : null,
          native_box: entry?.native, chromium_box: entry?.chromium,
          images: [`${dir}/out.png`, `${dir}/ref.png`, `${dir}/diff.png`],
          interpretation: 'Mismatch in a feature probe; attribution remains unconfirmed and may include inherited font/layout errors.' }, 'probe');
      }
    }
  }
  const { diff, ...pixelStats } = pixel;
  results.push({ fixture: name, suite: fixtures[name].suite, status: 'MEASURED', elements,
    mean_iou: elements.reduce((sum, e) => sum + e.iou, 0) / elements.length,
    max_edge_error_px: Math.max(...elements.map(e => e.max_edge_error_px)),
    geometry_match_ratio: elements.filter(e => e.max_edge_error_px <= 1).length / Math.max(native.elements.length, ref.elements.length),
    pixel: pixelStats, issues: errors });
}
const gaps = [...gapFixtures].map(([gap, names]) => ({ gap, fixture_count: names.size,
  max_edge_error_px: gapErrors.get(gap), fixtures: [...names].sort() }))
  .sort((a, b) => b.fixture_count - a.fixture_count || b.max_edge_error_px - a.max_edge_error_px || a.gap.localeCompare(b.gap));
const probeAssociations = [...probeFixtures].map(([gap, names]) => ({ gap,
  fixture_count: names.size, fixtures: [...names].sort() }))
  .sort((a, b) => b.fixture_count - a.fixture_count || a.gap.localeCompare(b.gap));
await mkdir('out', { recursive: true });
const environment = JSON.parse(await readFile('out/environment.json', 'utf8'));
await writeFile('out/report.json', JSON.stringify({ schema_version: 1, generated_at: new Date().toISOString(),
  environment, width: 412, viewport_height: 800, geometry_tolerance_px: 1,
  pixel_channel_threshold: 16, fixtures: results, gaps, probe_associations: probeAssociations }, null, 2) + '\n');
const screenResults = results.filter(r => r.suite === 'screen');
const propertyResults = results.filter(r => r.suite === 'probe');
const makeTable = rows => [
  '| Fixture | Mean IoU | Edges ≤1px | Max edge px | Pixel diff % |',
  '|---|---:|---:|---:|---:|',
  ...rows.map(r => '| ' + r.fixture + ' | ' + r.mean_iou.toFixed(4) +
    ' | ' + (100 * r.geometry_match_ratio).toFixed(1) + '% | ' +
    r.max_edge_error_px.toFixed(2) + ' | ' +
    (100 * r.pixel.changed_pixel_ratio).toFixed(2) + ' |'),
].join('\n');
const table = makeTable(screenResults);
const probesTable = makeTable(propertyResults);
const report = '# Week 2 measurements\n\n' +
  'Native: ' + environment.native_platform + '; Chromium: ' + environment.browser_platform + '.\n\n' +
  '## Screen fixtures\n\n' + table +
  '\n\n## Isolated property probes (not included in the 21-screen parity gate)\n\n' + probesTable +
  '\n\nTop measured screen geometry gaps:\n\n' +
  gaps.slice(0, 10).map(g => '- ' + g.gap + ': ' + g.fixture_count + '/' +
    screenResults.length + '; max edge ' + g.max_edge_error_px.toFixed(2) + 'px').join('\n') +
  '\n\nProperty associations and computed-style differences are diagnostics, not root-cause claims.\n' +
  '\n\nDetails and images: out/report.json and out/<fixture>/{out,ref,diff}.png.\n';
await writeFile('out/REPORT.md', report);
const reportPath = process.env.REPORT_PATH || 'REPORT.md';
const existing = await readFile(reportPath, 'utf8').catch(() => '');
const marker = '<!-- measurements -->';
await writeFile(reportPath, (existing.includes(marker) ? existing.split(marker)[0] : existing) + marker + '\n\n' + report);
console.log('Screen fixtures:\n' + table + '\n\nIsolated probes:\n' + probesTable);
