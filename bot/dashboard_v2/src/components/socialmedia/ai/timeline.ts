import type { AiTimeRange } from './api';

function ordered(kept: AiTimeRange[]): AiTimeRange[] {
  return [...kept].sort((a, b) => a.start_ms - b.start_ms);
}

export function removedSections(kept: AiTimeRange[], durationMs: number | null): AiTimeRange[] {
  const removed: AiTimeRange[] = [];
  let cursor = 0;
  for (const range of ordered(kept)) {
    if (range.start_ms > cursor) removed.push({ start_ms: cursor, end_ms: range.start_ms, description: 'Aus der bearbeiteten Fassung entfernt.' });
    cursor = Math.max(cursor, range.end_ms);
  }
  if (durationMs !== null && durationMs > cursor) removed.push({ start_ms: cursor, end_ms: durationMs, description: 'Aus der bearbeiteten Fassung entfernt.' });
  return removed;
}

export function sourceTimeMs(kept: AiTimeRange[], outputMs: number): number | null {
  let offset = 0;
  const ranges = ordered(kept);
  for (const range of ranges) {
    const length = range.end_ms - range.start_ms;
    if (outputMs < offset + length) return range.start_ms + Math.max(0, outputMs - offset);
    offset += length;
  }
  const last = ranges[ranges.length - 1];
  return last ? last.end_ms : null;
}
