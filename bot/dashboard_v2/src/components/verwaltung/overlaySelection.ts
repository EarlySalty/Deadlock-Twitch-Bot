export type CanvasRect = { x: number; y: number; width: number; height: number };
export type SnapHit = { axis: 'x' | 'y'; position: number };

export function selectionBounds(rects: CanvasRect[]): CanvasRect | null {
  if (!rects.length) return null;
  const x = Math.min(...rects.map(rect => rect.x));
  const y = Math.min(...rects.map(rect => rect.y));
  return {
    x, y,
    width: Math.max(...rects.map(rect => rect.x + rect.width)) - x,
    height: Math.max(...rects.map(rect => rect.y + rect.height)) - y,
  };
}

export function selectionIntersects(a: CanvasRect, b: CanvasRect): boolean {
  return a.x <= b.x + b.width && a.x + a.width >= b.x &&
    a.y <= b.y + b.height && a.y + a.height >= b.y;
}

export function toggleSelection<K>(selection: K[], key: K): K[] {
  return selection.includes(key) ? selection.filter(value => value !== key) : [...selection, key];
}

/** One bounded delta for every member; alignment never changes internal spacing. */
export function moveSelection(
  rects: CanvasRect[], targets: CanvasRect[],
  canvasWidth: number, canvasHeight: number,
  dx: number, dy: number, thresholdX = 0, thresholdY = 0,
): { dx: number; dy: number; hits: SnapHit[] } {
  const bounds = selectionBounds(rects);
  if (!bounds) return { dx: 0, dy: 0, hits: [] };
  const hits: SnapHit[] = [];
  const axisDelta = (axis: 'x' | 'y', size: 'width' | 'height', limit: number, raw: number, threshold: number) => {
    const min = -bounds[axis];
    const max = limit - bounds[axis] - bounds[size];
    const delta = Math.min(max, Math.max(min, Math.round(raw)));
    if (threshold <= 0) return delta;
    const anchors = (rect: CanvasRect) => [rect[axis], rect[axis] + rect[size] / 2, rect[axis] + rect[size]];
    const destinations = [0, limit / 2, limit, ...targets.flatMap(anchors)];
    let best: { delta: number; position: number; distance: number } | null = null;
    // Include each member's anchors as well as the group's outer bounds.
    for (const anchor of [...rects, bounds].flatMap(anchors)) {
      for (const position of destinations) {
        const candidate = Math.round(position - anchor);
        const distance = Math.abs(position - anchor - delta);
        if (distance <= threshold && candidate >= min && candidate <= max &&
          (!best || distance < best.distance)) best = { delta: candidate, position, distance };
      }
    }
    if (!best) return delta;
    hits.push({ axis, position: best.position });
    return best.delta;
  };
  return {
    dx: axisDelta('x', 'width', canvasWidth, dx, thresholdX),
    dy: axisDelta('y', 'height', canvasHeight, dy, thresholdY),
    hits,
  };
}
