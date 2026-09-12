type Props = {
  source: { x: number; y: number; width: number; height: number };
  canvasWidth: number;
  canvasHeight: number;
  scale: number;
};

/** Linien in Vorschau-Pixeln, Beschriftungen immer in echten OBS-Pixeln. */
export function OverlayCanvasGuides({ source, canvasWidth, canvasHeight, scale }: Props) {
  const { x, y, width, height } = source;
  const w = canvasWidth * scale;
  const h = canvasHeight * scale;
  const cx = (x + width / 2) * scale;
  const cy = (y + height / 2) * scale;
  const centeredX = Math.abs(x + width / 2 - canvasWidth / 2) <= 0.5;
  const centeredY = Math.abs(y + height / 2 - canvasHeight / 2) <= 0.5;
  const distances = [
    { x1: 0, y1: cy, x2: x * scale, y2: cy, value: x },
    { x1: (x + width) * scale, y1: cy, x2: w, y2: cy, value: canvasWidth - x - width },
    { x1: cx, y1: 0, x2: cx, y2: y * scale, value: y },
    { x1: cx, y1: (y + height) * scale, x2: cx, y2: h, value: canvasHeight - y - height },
  ];
  return (
    <svg data-testid="overlay-measurements" aria-hidden="true" className="pointer-events-none absolute inset-0 z-20" width="100%" height="100%">
      <line data-guide="center-x" x1={w / 2} x2={w / 2} y1={0} y2={h} stroke={centeredX ? '#4ade80' : '#d6b56c'} strokeOpacity={centeredX ? 1 : 0.45} strokeDasharray="4 4" />
      <line data-guide="center-y" x1={0} x2={w} y1={h / 2} y2={h / 2} stroke={centeredY ? '#4ade80' : '#d6b56c'} strokeOpacity={centeredY ? 1 : 0.45} strokeDasharray="4 4" />
      {distances.map(({ x1, y1, x2, y2, value }, index) => (
        <g key={index} data-distance={['left', 'right', 'top', 'bottom'][index]}>
          <line x1={x1} y1={y1} x2={x2} y2={y2} stroke="#ef7474" />
          <text x={Math.max(24, Math.min(w - 24, (x1 + x2) / 2))} y={Math.max(13, Math.min(h - 4, (y1 + y2) / 2 - 4))} textAnchor="middle" fill="#fff" stroke="#090a0d" strokeWidth={3} paintOrder="stroke" fontSize={11} fontFamily="monospace">{Math.round(value)} px</text>
        </g>
      ))}
      <text x={Math.max(45, Math.min(w - 45, cx))} y={Math.max(14, Math.min(h - 5, y * scale + 15))} textAnchor="middle" fill="#d6b56c" stroke="#090a0d" strokeWidth={3} paintOrder="stroke" fontSize={11} fontFamily="monospace">{width} × {height}</text>
    </svg>
  );
}
