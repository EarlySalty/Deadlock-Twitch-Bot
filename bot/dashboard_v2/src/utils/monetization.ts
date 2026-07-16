export function fmtDrop(n: number | null | undefined): string {
  if (n === null || n === undefined) return '-';
  if (n > 0) return `−${n.toFixed(1)}%`;
  if (n < 0) return `+${Math.abs(n).toFixed(1)}%`;
  return '0.0%';
}
