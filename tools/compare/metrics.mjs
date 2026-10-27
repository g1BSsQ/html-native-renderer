// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
export function geometry(a, b) {
  const area = r => Math.max(0, r.width) * Math.max(0, r.height);
  const overlap = Math.max(0, Math.min(a.x + a.width, b.x + b.width) - Math.max(a.x, b.x)) *
    Math.max(0, Math.min(a.y + a.height, b.y + b.height) - Math.max(a.y, b.y));
  const union = area(a) + area(b) - overlap;
  const maxEdgeError = Math.max(Math.abs(a.x - b.x), Math.abs(a.y - b.y),
    Math.abs(a.x + a.width - b.x - b.width), Math.abs(a.y + a.height - b.y - b.height));
  return { iou: union ? overlap / union : maxEdgeError === 0 ? 1 : 0, max_edge_error_px: maxEdgeError };
}

export function pixels(a, b) {
  const width = Math.max(a.width, b.width), height = Math.max(a.height, b.height);
  const diff = Buffer.alloc(width * height * 4);
  let changed = 0, totalError = 0;
  for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) {
    let max = 0;
    for (let c = 0; c < 3; c++) {
      const av = x < a.width && y < a.height ? a.data[(y * a.width + x) * 4 + c] : 255;
      const bv = x < b.width && y < b.height ? b.data[(y * b.width + x) * 4 + c] : 255;
      const error = Math.abs(av - bv);
      max = Math.max(max, error); totalError += error;
    }
    if (max > 16) changed++;
    const offset = (y * width + x) * 4;
    diff[offset] = max; diff[offset + 1] = 0; diff[offset + 2] = max; diff[offset + 3] = 255;
  }
  return { width, height, diff, changed_pixel_ratio: changed / (width * height),
    mean_absolute_channel_error: totalError / (width * height * 3) };
}
