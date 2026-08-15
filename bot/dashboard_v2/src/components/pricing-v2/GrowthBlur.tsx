type Point = { x: number; y: number };

export function GrowthBlur({
  values,
  days,
  peak,
}: {
  values: number[];
  days: number | null;
  peak: number | null;
}) {
  const width = 320;
  const height = 88;
  const series = values.length > 1 ? values : [0, 0];
  const max = Math.max(...series, 1);
  const points: Point[] = series.map((value, index) => ({
    x: (index / (series.length - 1)) * width,
    y: height - (value / max) * (height - 8) - 4,
  }));
  const d = points
    .map((point, index) => `${index === 0 ? 'M' : 'L'} ${point.x.toFixed(1)} ${point.y.toFixed(1)}`)
    .join(' ');

  return (
    <div>
      <svg
        viewBox={`0 0 ${width} ${height}`}
        className="h-24 w-full"
        style={{ filter: 'blur(6px)' }}
        aria-hidden
      >
        <path d={d} fill="none" stroke="currentColor" strokeWidth="3" className="text-white/70" />
      </svg>
      <p className="mt-3 text-sm text-white/55">
        {days ?? '—'} Tage · Ø {avgLabel(series)} · Peak {peak ?? '—'}
      </p>
    </div>
  );
}

function avgLabel(values: number[]): string {
  if (!values.length) return '—';
  const avg = values.reduce((sum, value) => sum + value, 0) / values.length;
  return Number.isFinite(avg) ? String(Math.round(avg)) : '—';
}
