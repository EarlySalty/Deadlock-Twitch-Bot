export type CanvasRect = { x: number; y: number; width: number; height: number };

export function toggleSelection<K>(selection: K[], key: K): K[] {
  return selection.includes(key) ? selection.filter(value => value !== key) : [...selection, key];
}

/** Die ganze Gruppe erhält dieselbe begrenzte Verschiebung; Abstände bleiben erhalten. */
export function boundedGroupDelta(rects: CanvasRect[], width: number, height: number, dx: number, dy: number) {
  if (!rects.length) return { dx: 0, dy: 0 };
  const left = Math.min(...rects.map(rect => rect.x));
  const top = Math.min(...rects.map(rect => rect.y));
  const right = Math.max(...rects.map(rect => rect.x + rect.width));
  const bottom = Math.max(...rects.map(rect => rect.y + rect.height));
  return {
    dx: Math.min(width - right, Math.max(-left, Math.round(dx))),
    dy: Math.min(height - bottom, Math.max(-top, Math.round(dy))),
  };
}
