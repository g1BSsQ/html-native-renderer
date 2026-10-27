// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
import { readFile } from 'node:fs/promises';
import path from 'node:path';

export const AREAS = [
  'css/css-flexbox', 'css/css-grid', 'css/css-box',
  'css/CSS2/box-display', 'css/CSS2/margin-padding-clear', 'css/CSS2/borders',
  'css/css-backgrounds', 'css/css-color', 'css/css-text', 'css/css-fonts',
  'css/css-position', 'css/css-overflow', 'css/css-sizing', 'css/css-values',
  'css/css-transforms', 'css/css-masking', 'css/selectors'
];

export class Excluded extends Error {
  constructor(reason) { super(reason); this.name = 'Excluded'; }
}

export function attributes(tag) {
  const out = {};
  const start = tag.match(/^<[\w:-]+\b/);
  const rest = start ? tag.slice(start[0].length) : tag;
  const rx = /([^\s=/>]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+)))?/g;
  for (const m of rest.matchAll(rx)) out[m[1].toLowerCase()] = m[2] ?? m[3] ?? m[4] ?? '';
  return out;
}

export function references(html) {
  const refs = [];
  const withoutComments = html.replace(/<!--[\s\S]*?-->/g, '');
  for (const match of withoutComments.matchAll(/<link\b[^>]*>/gi)) {
    const a = attributes(match[0]);
    const rels = (a.rel || '').toLowerCase().split(/\s+/);
    const kind = rels.includes('match') ? 'match' : rels.includes('mismatch') ? 'mismatch' : null;
    if (kind) refs.push({ kind, href: a.href || '' });
  }
  return refs;
}

export function selection(html) {
  const refs = references(html);
  if (!refs.length) return null;
  const text = html.replace(/<!--[\s\S]*?-->/g, '');
  if (/<script\b/i.test(text)) return { refs, reason: 'script_element' };
  if (/reftest-wait/i.test(text)) return { refs, reason: 'reftest_wait' };
  if (/testharness\.js/i.test(text)) return { refs, reason: 'testharness' };
  if (/<(?:iframe|frame|object|embed|video|audio|canvas)\b/i.test(text))
    return { refs, reason: 'unsupported_embedded_or_dynamic_resource' };
  if (/<(?:animate|animatetransform|set)\b/i.test(text) ||
      /@keyframes\b|\banimation(?:-[\w-]+)?\s*:|\btransition(?:-[\w-]+)?\s*:/i.test(text))
    return { refs, reason: 'animation_or_transition' };
  if (/<meta\b[^>]*http-equiv\s*=\s*["']?refresh\b/i.test(text))
    return { refs, reason: 'refresh' };
  if (/<[\w:-]+\b[^>]*\s+on[a-z]+\s*=/i.test(text))
    return { refs, reason: 'event_handler' };
  if (/\bsrcset\s*=/i.test(text)) return { refs, reason: 'responsive_resource_selection' };
  if (/<base\b/i.test(text)) return { refs, reason: 'document_base_url' };
  const remote = value => /^(?:https?:|\/\/|file:|javascript:)/i.test(value);
  for (const tag of text.match(/<(?:img|image|input|source|link)\b[^>]*>/gi) || []) {
    const attrs = attributes(tag);
    const name = /^<(\w+)/.exec(tag)[1].toLowerCase();
    const rels = (attrs.rel || '').toLowerCase().split(/\s+/);
    const isResource = name !== 'link' ||
      rels.some(r => ['stylesheet', 'match', 'mismatch', 'preload', 'prefetch'].includes(r));
    if (isResource && remote(attrs.src || attrs.href || attrs['xlink:href'] || ''))
      return { refs, reason: 'external_network_resource' };
  }
  if (/url\(\s*['"]?\s*(?:https?:|\/\/|file:)/i.test(text) ||
      /@import\s+(?:url\()?\s*['"]?\s*(?:https?:|\/\/)/i.test(text))
    return { refs, reason: 'external_network_resource' };
  if (refs.some(r => !r.href)) return { refs, reason: 'reference_without_href' };
  // Multiple reference graphs need WPT's reference-graph semantics; do not guess.
  if (refs.length !== 1) return { refs, reason: 'multiple_references' };
  const pathname = refs[0].href.toLowerCase().split(/[?#]/)[0];
  if (!/\.(?:html?|xht|xhtml)$/.test(pathname))
    return { refs, reason: 'non_html_reference' };
  return { refs, reason: null };
}

function range(s) {
  const m = /^(\d+)(?:-(\d+))?$/.exec(s.trim());
  if (!m) throw new Excluded('unsupported_fuzzy_rule');
  const lo = Number(m[1]), hi = Number(m[2] ?? m[1]);
  if (hi < lo) throw new Excluded('unsupported_fuzzy_rule');
  return [lo, hi];
}

export function fuzzyRule(html, reference) {
  let fallback = null, specific = null;
  for (const tag of html.match(/<meta\b[^>]*>/gi) || []) {
    const a = attributes(tag);
    if ((a.name || '').toLowerCase() !== 'fuzzy') continue;
    const content = (a.content || '').trim();
    const colon = content.indexOf(':');
    const ref = colon < 0 ? null : content.slice(0, colon).trim();
    const payload = colon < 0 ? content : content.slice(colon + 1).trim();
    const values = payload.split(';').map(v => v.trim());
    if (values.length !== 2) throw new Excluded('unsupported_fuzzy_rule');
    const named = values.some(v => /=/.test(v));
    let maxDifference, totalPixels;
    if (named) {
      const entries = {};
      for (const value of values) {
        const part = /^(maxDifference|totalPixels)\s*=\s*(\d+(?:-\d+)?)$/i.exec(value);
        if (!part) throw new Excluded('unsupported_fuzzy_rule');
        const key = part[1].toLowerCase();
        if (entries[key]) throw new Excluded('unsupported_fuzzy_rule');
        entries[key] = range(part[2]);
      }
      if (!entries.maxdifference || !entries.totalpixels) throw new Excluded('unsupported_fuzzy_rule');
      maxDifference = entries.maxdifference;
      totalPixels = entries.totalpixels;
    } else {
      maxDifference = range(values[0]);
      totalPixels = range(values[1]);
    }
    if (maxDifference[1] > 255) throw new Excluded('unsupported_fuzzy_rule');
    const rule = { maxDifference, totalPixels };
    if (ref === null) fallback = rule;
    else if (path.posix.normalize(ref) === path.posix.normalize(reference)) specific = rule;
  }
  return specific || fallback || { maxDifference: [0, 0], totalPixels: [0, 0] };
}

export function comparePixels(actual, reference, rule, kind = 'match') {
  const dimensionsMatch = actual.width === reference.width && actual.height === reference.height;
  let maxDifference = 0, totalPixels = 0;
  if (dimensionsMatch) {
    for (let i = 0; i < actual.data.length; i += 4) {
      // WPT's fuzzy metric is the maximum difference in RGB, not alpha.
      const difference = Math.max(
        Math.abs(actual.data[i] - reference.data[i]),
        Math.abs(actual.data[i + 1] - reference.data[i + 1]),
        Math.abs(actual.data[i + 2] - reference.data[i + 2])
      );
      if (difference > 0) totalPixels++;
      maxDifference = Math.max(maxDifference, difference);
    }
  } else {
    maxDifference = null;
    totalPixels = null;
  }
  const within = dimensionsMatch &&
    maxDifference >= rule.maxDifference[0] && maxDifference <= rule.maxDifference[1] &&
    totalPixels >= rule.totalPixels[0] && totalPixels <= rule.totalPixels[1];
  return { pass: kind === 'mismatch' ? !within : within,
    dimensions_match: dimensionsMatch, max_difference: maxDifference,
    total_pixels: totalPixels, fuzzy: rule, relation: kind };
}

export function localPath(root, from, href) {
  const url = href.trim().replace(/&amp;/g, '&');
  if (url.startsWith('#') || /^data:/i.test(url)) return null;
  if (/^(?:[a-z][\w+.-]*:|\/\/)/i.test(url)) throw new Excluded('external_or_unsafe_resource');
  if (/[?#]/.test(url) || /\\/.test(url)) throw new Excluded('resource_with_query_or_fragment');
  let decoded;
  try { decoded = decodeURIComponent(url); }
  catch { throw new Excluded('invalid_resource_url'); }
  if (decoded.includes('\0')) throw new Excluded('invalid_resource_url');
  const relative = path.posix.normalize(decoded.startsWith('/') ?
    decoded.slice(1) : path.posix.join(path.posix.dirname(from), decoded));
  if (relative === '..' || relative.startsWith('../') || path.posix.isAbsolute(relative))
    throw new Excluded('resource_outside_checkout');
  return relative;
}

const MIME = new Map(Object.entries({
  '.png': 'image/png', '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg',
  '.svg': 'image/svg+xml', '.webp': 'image/webp', '.bmp': 'image/bmp',
  '.woff': 'font/woff', '.woff2': 'font/woff2', '.ttf': 'font/ttf',
  '.otf': 'font/otf', '.ico': 'image/x-icon'
}));

async function resourceUri(root, from, url, font) {
  if (/^data:/i.test(url)) {
    if (/^data:image\/(?:gif|apng)[;,]/i.test(url)) throw new Excluded('potentially_animated_image');
    if (font) throw new Excluded('font_not_from_wpt_fonts');
    return url;
  }
  if (url.startsWith('#')) {
    if (font) throw new Excluded('font_not_from_wpt_fonts');
    return url;
  }
  const relative = localPath(root, from, url);
  if (font && !relative.startsWith('fonts/')) throw new Excluded('font_not_from_wpt_fonts');
  const ext = path.posix.extname(relative).toLowerCase();
  if (ext === '.gif' || ext === '.apng') throw new Excluded('potentially_animated_image');
  const mime = MIME.get(ext);
  if (!mime) throw new Excluded('unsupported_resource_type:' + ext);
  let bytes;
  try { bytes = await readFile(path.join(root, relative)); }
  catch (e) {
    if (e.code === 'ENOENT') throw new Excluded('missing_checkout_resource:' + relative);
    throw e;
  }
  return 'data:' + mime + ';base64,' + bytes.toString('base64');
}

async function replaceAsync(text, regex, substitute) {
  let result = '', pos = 0;
  for (const match of text.matchAll(regex)) {
    result += text.slice(pos, match.index) + await substitute(match);
    pos = match.index + match[0].length;
  }
  return result + text.slice(pos);
}

async function cssText(root, from, input, seen = new Set()) {
  let css = input.replace(/\/\*[\s\S]*?\*\//g, '');
  if (/@keyframes\b|\banimation(?:-[\w-]+)?\s*:|\btransition(?:-[\w-]+)?\s*:/i.test(css))
    throw new Excluded('animation_or_transition');
  if (/\b(?:matrix3d|translate3d|translatez|scale3d|scalez|rotate3d|rotatex|rotatey|perspective|transform-style|backface-visibility)\b/i.test(css))
    throw new Excluded('transform_3d');
  css = await replaceAsync(css, /@import\s+(?:url\(\s*(['"]?)(.*?)\1\s*\)|(['"])(.*?)\3)\s*([^;]*);/gi, async m => {
    const url = m[2] || m[4], relative = localPath(root, from, url);
    if (!relative || !relative.endsWith('.css')) throw new Excluded('unsupported_css_import');
    if (seen.has(relative)) throw new Excluded('cyclic_css_import');
    const nested = await loadCss(root, relative, new Set([...seen, relative]));
    const media = (m[5] || '').trim();
    return media ? '@media ' + media + ' {\n' + nested + '\n}' : nested;
  });
  if (/@import\b/i.test(css)) throw new Excluded('unsupported_css_import');
  const faces = [...css.matchAll(/@font-face\s*\{[^}]*\}/gi)]
    .map(m => [m.index, m.index + m[0].length]);
  for (const [start, end] of faces) {
    if (/\blocal\s*\(/i.test(css.slice(start, end))) throw new Excluded('non_wpt_local_font');
  }
  return replaceAsync(css, /url\(\s*(['"]?)(.*?)\1\s*\)/gi, async m => {
    const font = faces.some(([start, end]) => m.index >= start && m.index < end);
    const uri = await resourceUri(root, from, m[2], font);
    return 'url("' + uri + '")';
  });
}

async function loadCss(root, relative, seen) {
  let css;
  try { css = await readFile(path.join(root, relative), 'utf8'); }
  catch (e) {
    if (e.code === 'ENOENT') throw new Excluded('missing_checkout_resource:' + relative);
    throw e;
  }
  return cssText(root, relative, css, seen);
}

export async function prepareHtml(root, relative, original) {
  let html = original;
  html = await replaceAsync(html, /<style\b([^>]*)>([\s\S]*?)<\/style\s*>/gi, async m =>
    '<style' + m[1] + '>' + await cssText(root, relative, m[2]) + '</style>');
  html = await replaceAsync(html, /<link\b[^>]*>/gi, async m => {
    const a = attributes(m[0]), rel = (a.rel || '').toLowerCase().split(/\s+/);
    if (rel.includes('stylesheet')) {
      const href = localPath(root, relative, a.href || '');
      if (!href || !href.endsWith('.css')) throw new Excluded('unsupported_stylesheet');
      const css = await loadCss(root, href, new Set([href]));
      const media = a.media ? '@media ' + a.media + '{\n' + css + '\n}' : css;
      return '<style>' + media + '</style>';
    }
    if (rel.some(r => ['match', 'mismatch', 'author', 'help', 'icon'].includes(r))) return '';
    if (rel.some(r => ['preload', 'prefetch', 'modulepreload'].includes(r)))
      throw new Excluded('resource_preload');
    return m[0];
  });
  html = await replaceAsync(html, /<(?:img|image|source|input)\b[^>]*>/gi, async m => {
    let tag = m[0];
    tag = await replaceAsync(tag, /\b(src|href|xlink:href)\s*=\s*(["'])(.*?)\2/gi, async a => {
      const uri = await resourceUri(root, relative, a[3], false);
      return a[1] + '=' + a[2] + uri + a[2];
    });
    return tag;
  });
  html = await replaceAsync(html, /<[\w:-]+\b[^>]*\bstyle\s*=\s*(["'])(.*?)\1[^>]*>/gi,
    async m => m[0].replace(/(\bstyle\s*=\s*)(["'])(.*?)\2/i, (_full, key, quote, css) => {
      // Style attribute URLs are uncommon. Reject them rather than silently fetching.
      if (/url\s*\(|@import/i.test(css)) throw new Excluded('inline_style_resource');
      return key + quote + css + quote;
    }));
  return html;
}

export function cssProperties(html) {
  const names = new Set();
  const css = (html.match(/<style\b[^>]*>[\s\S]*?<\/style>/gi) || []).join('\n') + '\n' +
    [...html.matchAll(/\bstyle\s*=\s*(["'])(.*?)\1/gi)].map(m => m[2]).join('\n');
  for (const m of css.matchAll(/(?:^|[;{])\s*(--[\w-]+|[a-z-]+)\s*:/gim))
    names.add(m[1].toLowerCase());
  return [...names].sort();
}
