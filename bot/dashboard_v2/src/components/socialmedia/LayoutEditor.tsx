import { useEffect, useMemo, useRef, useState } from 'react';
import { Camera, Layers, Maximize2, Save, RotateCcw, EyeOff, Eye } from 'lucide-react';
import type { LayoutBox, LayoutPayload, LayoutMode } from '@/types/socialMedia';
import { DEFAULT_LAYOUT, DEFAULT_SOURCE_HEIGHT, DEFAULT_SOURCE_WIDTH } from '@/types/socialMedia';

type DragMode = 'move' | 'resize-tl' | 'resize-tr' | 'resize-bl' | 'resize-br';

type BoxId = 'game_crop' | 'cam_crop' | 'cam_position';

interface DragState {
  pointerId: number;
  boxId: BoxId;
  mode: DragMode;
  start: { x: number; y: number };
  startBox: LayoutBox;
  containerWidth: number;
  containerHeight: number;
  sourceWidth: number;
  sourceHeight: number;
}

interface LayoutEditorProps {
  initialLayout?: LayoutPayload;
  isSaving?: boolean;
  onSave: (layout: LayoutPayload) => void;
  onReset?: () => void;
  saveLabel?: string;
  resetLabel?: string;
}

function clampBox(box: LayoutBox, sw: number, sh: number, minSize = 80): LayoutBox {
  const w = Math.max(minSize, Math.min(box.w, sw));
  const h = Math.max(minSize, Math.min(box.h, sh));
  const x = Math.max(0, Math.min(box.x, sw - w));
  const y = Math.max(0, Math.min(box.y, sh - h));
  return { x, y, w, h };
}

function formatBox(box: LayoutBox): string {
  return `${Math.round(box.w)}×${Math.round(box.h)} @ (${Math.round(box.x)},${Math.round(box.y)})`;
}

interface SourcePreviewProps {
  layout: LayoutPayload;
  camEnabled: boolean;
  mode: LayoutMode;
  onChange: (next: LayoutPayload) => void;
  selectedBox: BoxId;
  onSelectBox: (id: BoxId) => void;
}

function SourcePreview({ layout, camEnabled, mode, onChange, selectedBox, onSelectBox }: SourcePreviewProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const dragRef = useRef<DragState | null>(null);

  const handlePointerDown = (e: React.PointerEvent, boxId: BoxId, mode: DragMode) => {
    if (!containerRef.current) return;
    e.preventDefault();
    e.stopPropagation();
    const rect = containerRef.current.getBoundingClientRect();
    const startBox = layout[boxId];
    dragRef.current = {
      pointerId: e.pointerId,
      boxId,
      mode,
      start: { x: e.clientX, y: e.clientY },
      startBox: { ...startBox },
      containerWidth: rect.width,
      containerHeight: rect.height,
      sourceWidth: layout.source.width,
      sourceHeight: layout.source.height,
    };
    onSelectBox(boxId);
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  };

  const handlePointerMove = (e: React.PointerEvent) => {
    const state = dragRef.current;
    if (!state || state.pointerId !== e.pointerId) return;
    const dxPx = e.clientX - state.start.x;
    const dyPx = e.clientY - state.start.y;
    const sx = state.sourceWidth / state.containerWidth;
    const sy = state.sourceHeight / state.containerHeight;
    const dxSrc = dxPx * sx;
    const dySrc = dyPx * sy;

    let next: LayoutBox = { ...state.startBox };
    switch (state.mode) {
      case 'move':
        next.x = state.startBox.x + dxSrc;
        next.y = state.startBox.y + dySrc;
        break;
      case 'resize-br':
        next.w = state.startBox.w + dxSrc;
        next.h = state.startBox.h + dySrc;
        break;
      case 'resize-tr':
        next.y = state.startBox.y + dySrc;
        next.w = state.startBox.w + dxSrc;
        next.h = state.startBox.h - dySrc;
        break;
      case 'resize-bl':
        next.x = state.startBox.x + dxSrc;
        next.w = state.startBox.w - dxSrc;
        next.h = state.startBox.h + dySrc;
        break;
      case 'resize-tl':
        next.x = state.startBox.x + dxSrc;
        next.y = state.startBox.y + dySrc;
        next.w = state.startBox.w - dxSrc;
        next.h = state.startBox.h - dySrc;
        break;
    }
    next = clampBox(next, state.sourceWidth, state.sourceHeight);
    onChange({ ...layout, [state.boxId]: next });
  };

  const handlePointerUp = (e: React.PointerEvent) => {
    if (dragRef.current && dragRef.current.pointerId === e.pointerId) {
      dragRef.current = null;
    }
  };

  const renderBox = (boxId: BoxId, color: 'orange' | 'teal' | 'purple', label: string) => {
    const box = layout[boxId];
    const left = (box.x / layout.source.width) * 100;
    const top = (box.y / layout.source.height) * 100;
    const width = (box.w / layout.source.width) * 100;
    const height = (box.h / layout.source.height) * 100;
    const isSelected = selectedBox === boxId;

    const borderColor =
      color === 'orange'
        ? 'rgba(255,122,24,0.95)'
        : color === 'teal'
        ? 'rgba(16,183,173,0.95)'
        : 'rgba(168,85,247,0.95)';
    const fillColor =
      color === 'orange'
        ? 'rgba(255,122,24,0.18)'
        : color === 'teal'
        ? 'rgba(16,183,173,0.18)'
        : 'rgba(168,85,247,0.18)';

    return (
      <div
        key={boxId}
        onPointerDown={(e) => handlePointerDown(e, boxId, 'move')}
        className={`absolute cursor-move select-none transition-shadow ${isSelected ? 'z-20' : 'z-10'}`}
        style={{
          left: `${left}%`,
          top: `${top}%`,
          width: `${width}%`,
          height: `${height}%`,
          border: `2px solid ${borderColor}`,
          background: fillColor,
          boxShadow: isSelected
            ? `0 0 0 3px ${borderColor}, 0 8px 24px rgba(0,0,0,0.45)`
            : `0 4px 14px rgba(0,0,0,0.3)`,
          backdropFilter: 'blur(2px)',
        }}
      >
        <div className="absolute -top-7 left-0 flex items-center gap-1.5 text-[10px] font-bold uppercase tracking-[0.14em] text-white px-2 py-0.5 rounded-md backdrop-blur-md"
             style={{ background: borderColor }}>
          {label}
        </div>
        <div className="absolute -bottom-6 right-0 text-[10px] font-mono text-white/80 bg-black/55 px-1.5 py-0.5 rounded">
          {formatBox(box)}
        </div>
        {/* Resize handles */}
        {(['resize-tl', 'resize-tr', 'resize-bl', 'resize-br'] as DragMode[]).map((mode) => {
          const isTop = mode.includes('tl') || mode.includes('tr');
          const isLeft = mode.includes('tl') || mode.includes('bl');
          const cursor =
            mode === 'resize-tl' || mode === 'resize-br'
              ? 'cursor-nwse-resize'
              : 'cursor-nesw-resize';
          return (
            <div
              key={mode}
              onPointerDown={(e) => handlePointerDown(e, boxId, mode)}
              className={`absolute w-3.5 h-3.5 ${cursor}`}
              style={{
                top: isTop ? -7 : 'auto',
                bottom: !isTop ? -7 : 'auto',
                left: isLeft ? -7 : 'auto',
                right: !isLeft ? -7 : 'auto',
                background: borderColor,
                borderRadius: 4,
                border: '2px solid #07151d',
              }}
            />
          );
        })}
      </div>
    );
  };

  return (
    <div
      ref={containerRef}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerUp}
      onPointerCancel={handlePointerUp}
      className="relative w-full select-none rounded-2xl overflow-hidden"
      style={{
        aspectRatio: `${layout.source.width} / ${layout.source.height}`,
        background:
          'repeating-linear-gradient(45deg, rgba(255,255,255,0.04) 0 12px, transparent 12px 24px), linear-gradient(135deg,#0e2a3a,#07151d)',
        border: '1px solid rgba(194,221,240,0.18)',
      }}
    >
      {/* Source label */}
      <div className="absolute top-3 left-3 text-[10px] font-bold uppercase tracking-[0.18em] text-white/60 px-2 py-1 rounded-md bg-black/40 backdrop-blur-md z-30">
        Twitch-Quelle 16:9 · {layout.source.width}×{layout.source.height}
      </div>

      {/* Boxes */}
      {renderBox('game_crop', 'orange', 'Game')}
      {camEnabled && renderBox('cam_crop', 'teal', 'Cam')}
      {camEnabled && mode === 'stacked' && renderBox('cam_position', 'purple', 'Cam-Position')}
    </div>
  );
}

interface VerticalPreviewProps {
  layout: LayoutPayload;
  camEnabled: boolean;
  mode: LayoutMode;
}

function VerticalPreview({ layout, camEnabled, mode }: VerticalPreviewProps) {
  // Simulates 1080×1920 composition. Game-Crop wird auf 9:16 gebracht.
  // Mode 'pip': Game füllt das gesamte Frame; Cam-Crop sitzt rechts oben als kleiner PiP-Block.
  // Mode 'stacked': cam_position bestimmt den oberen Cam-Bereich, Game darunter.

  const targetW = 1080;
  const targetH = 1920;

  // PiP-Cam-Box: feste Position rechts oben in einer 9:16-Vorschau (Cam-Crop wird in einen 360×360 PiP gemapped).
  const pipW = 360;
  const pipH = 360;
  const pipMarginX = 36;
  const pipMarginY = 60;

  if (mode === 'pip') {
    return (
      <div
        className="relative mx-auto rounded-2xl overflow-hidden border"
        style={{
          width: '100%',
          maxWidth: 320,
          aspectRatio: `${targetW} / ${targetH}`,
          borderColor: 'rgba(194,221,240,0.18)',
          background: 'linear-gradient(160deg,#102635,#07151d)',
        }}
      >
        {/* Game fill */}
        <div
          className="absolute inset-0 flex items-center justify-center"
          style={{
            background:
              'repeating-linear-gradient(45deg, rgba(255,122,24,0.18) 0 14px, rgba(255,122,24,0.06) 14px 28px)',
          }}
        >
          <span className="text-white/80 text-xs font-bold uppercase tracking-[0.18em]">Game (gecroppt)</span>
        </div>
        {/* PiP cam */}
        {camEnabled && (
          <div
            className="absolute rounded-xl overflow-hidden flex items-center justify-center"
            style={{
              right: `${(pipMarginX / targetW) * 100}%`,
              top: `${(pipMarginY / targetH) * 100}%`,
              width: `${(pipW / targetW) * 100}%`,
              aspectRatio: `${pipW} / ${pipH}`,
              background:
                'repeating-linear-gradient(45deg, rgba(16,183,173,0.30) 0 10px, rgba(16,183,173,0.10) 10px 20px)',
              border: '2px solid rgba(16,183,173,0.95)',
              boxShadow: '0 6px 22px rgba(16,183,173,0.35)',
            }}
          >
            <span className="text-white text-[10px] font-bold uppercase tracking-[0.16em]">Cam</span>
          </div>
        )}
        <div className="absolute bottom-2 left-2 text-[10px] font-mono text-white/60 bg-black/45 px-1.5 py-0.5 rounded">
          1080×1920 · PiP
        </div>
      </div>
    );
  }

  // stacked mode
  const camRatio = camEnabled
    ? Math.max(0.18, Math.min(0.5, layout.cam_position.h / layout.source.height))
    : 0;
  return (
    <div
      className="relative mx-auto rounded-2xl overflow-hidden border flex flex-col"
      style={{
        width: '100%',
        maxWidth: 320,
        aspectRatio: `${targetW} / ${targetH}`,
        borderColor: 'rgba(194,221,240,0.18)',
        background: 'linear-gradient(160deg,#102635,#07151d)',
      }}
    >
      {camEnabled && (
        <div
          className="relative w-full flex items-center justify-center"
          style={{
            height: `${camRatio * 100}%`,
            background:
              'repeating-linear-gradient(45deg, rgba(16,183,173,0.30) 0 12px, rgba(16,183,173,0.10) 12px 24px)',
            borderBottom: '1px solid rgba(255,255,255,0.08)',
          }}
        >
          <span className="text-white text-xs font-bold uppercase tracking-[0.18em]">Cam</span>
        </div>
      )}
      <div
        className="relative flex-1 flex items-center justify-center"
        style={{
          background:
            'repeating-linear-gradient(45deg, rgba(255,122,24,0.18) 0 14px, rgba(255,122,24,0.06) 14px 28px)',
        }}
      >
        <span className="text-white text-xs font-bold uppercase tracking-[0.18em]">Game</span>
      </div>
      <div className="absolute bottom-2 left-2 text-[10px] font-mono text-white/60 bg-black/45 px-1.5 py-0.5 rounded">
        1080×1920 · Stacked
      </div>
    </div>
  );
}

export function LayoutEditor({
  initialLayout,
  isSaving,
  onSave,
  onReset,
  saveLabel = 'Als Standard speichern',
  resetLabel = 'Zurücksetzen',
}: LayoutEditorProps) {
  const base = initialLayout ?? DEFAULT_LAYOUT;
  const [layout, setLayout] = useState<LayoutPayload>(base);
  const camEnabled = layout.cam_enabled;
  const mode = layout.mode;
  const setCamEnabled = (next: boolean) => setLayout((l) => ({ ...l, cam_enabled: next }));
  const setMode = (next: LayoutMode) => setLayout((l) => ({ ...l, mode: next }));
  const [selectedBox, setSelectedBox] = useState<BoxId>('game_crop');

  // Sync when initialLayout changes (e.g. streamer switch).
  useEffect(() => {
    if (initialLayout) setLayout(initialLayout);
  }, [initialLayout]);

  const isDirty = useMemo(() => {
    return JSON.stringify(layout) !== JSON.stringify(base);
  }, [layout, base]);

  const handleReset = () => {
    setLayout(base);
    onReset?.();
  };

  const handleResetToDefault = () => {
    setLayout({
      version: 1,
      source: { width: DEFAULT_SOURCE_WIDTH, height: DEFAULT_SOURCE_HEIGHT },
      game_crop: { ...DEFAULT_LAYOUT.game_crop },
      cam_crop: { ...DEFAULT_LAYOUT.cam_crop },
      cam_position: { ...DEFAULT_LAYOUT.cam_position },
      cam_enabled: true,
      mode: 'pip',
    });
  };

  return (
    <div className="panel-card rounded-2xl p-5 md:p-6 space-y-5">
      {/* Toolbar */}
      <div className="flex flex-wrap items-center gap-3">
        <div className="flex items-center gap-2 text-[11px] font-bold uppercase tracking-[0.16em] text-white/70">
          <Maximize2 className="w-4 h-4 text-orange" /> Layout-Editor
        </div>

        <div className="ml-auto flex flex-wrap items-center gap-2">
          {/* Mode toggle */}
          <div className="inline-flex rounded-xl border border-border bg-bg/60 p-1 text-xs font-semibold">
            <button
              type="button"
              onClick={() => setMode('pip')}
              className={`px-3 py-1.5 rounded-lg transition ${
                mode === 'pip' ? 'bg-orange text-white shadow-[0_4px_14px_rgba(255,122,24,0.35)]' : 'text-text-secondary hover:text-white'
              }`}
            >
              <span className="inline-flex items-center gap-1.5">
                <Layers className="w-3.5 h-3.5" /> PiP
              </span>
            </button>
            <button
              type="button"
              onClick={() => setMode('stacked')}
              className={`px-3 py-1.5 rounded-lg transition ${
                mode === 'stacked' ? 'bg-orange text-white shadow-[0_4px_14px_rgba(255,122,24,0.35)]' : 'text-text-secondary hover:text-white'
              }`}
            >
              <span className="inline-flex items-center gap-1.5">
                <Layers className="w-3.5 h-3.5 rotate-90" /> Stacked
              </span>
            </button>
          </div>

          {/* Cam toggle */}
          <button
            type="button"
            onClick={() => setCamEnabled(!camEnabled)}
            className={`inline-flex items-center gap-2 px-3 py-2 rounded-xl text-xs font-semibold border transition ${
              camEnabled
                ? 'bg-teal/15 text-teal border-teal/40'
                : 'bg-bg/60 text-text-secondary border-border hover:text-white'
            }`}
          >
            {camEnabled ? <Eye className="w-3.5 h-3.5" /> : <EyeOff className="w-3.5 h-3.5" />}
            Cam {camEnabled ? 'an' : 'aus'}
          </button>
        </div>
      </div>

      {/* Editor + Preview */}
      <div className="grid grid-cols-1 lg:grid-cols-[minmax(0,1fr)_auto] gap-6">
        {/* Source preview */}
        <div className="space-y-3">
          <div className="text-xs text-text-secondary uppercase tracking-[0.16em] font-bold">Quelle (Twitch-Frame)</div>
          <SourcePreview
            layout={layout}
            camEnabled={camEnabled}
            mode={mode}
            onChange={setLayout}
            selectedBox={selectedBox}
            onSelectBox={setSelectedBox}
          />
          <div className="grid grid-cols-3 gap-2 text-[11px]">
            <button
              type="button"
              onClick={() => setSelectedBox('game_crop')}
              className={`px-2.5 py-2 rounded-lg border font-semibold uppercase tracking-[0.14em] ${
                selectedBox === 'game_crop'
                  ? 'border-orange/70 text-orange bg-orange/10'
                  : 'border-border text-text-secondary hover:text-white'
              }`}
            >
              Game
            </button>
            <button
              type="button"
              disabled={!camEnabled}
              onClick={() => setSelectedBox('cam_crop')}
              className={`px-2.5 py-2 rounded-lg border font-semibold uppercase tracking-[0.14em] ${
                selectedBox === 'cam_crop'
                  ? 'border-teal/70 text-teal bg-teal/10'
                  : 'border-border text-text-secondary hover:text-white'
              } ${!camEnabled ? 'opacity-40 cursor-not-allowed' : ''}`}
            >
              <span className="inline-flex items-center justify-center gap-1.5"><Camera className="w-3 h-3" /> Cam</span>
            </button>
            <button
              type="button"
              disabled={!camEnabled || mode !== 'stacked'}
              onClick={() => setSelectedBox('cam_position')}
              className={`px-2.5 py-2 rounded-lg border font-semibold uppercase tracking-[0.14em] ${
                selectedBox === 'cam_position'
                  ? 'border-accent/70 text-accent bg-accent/10'
                  : 'border-border text-text-secondary hover:text-white'
              } ${!camEnabled || mode !== 'stacked' ? 'opacity-40 cursor-not-allowed' : ''}`}
            >
              Cam-Pos
            </button>
          </div>
        </div>

        {/* Vertical preview */}
        <div className="space-y-3 lg:w-[340px]">
          <div className="text-xs text-text-secondary uppercase tracking-[0.16em] font-bold">Vorschau 9:16</div>
          <VerticalPreview layout={layout} camEnabled={camEnabled} mode={mode} />
          <div className="text-[11px] text-text-secondary leading-relaxed">
            Live-Schema. Tatsächliche Komposition rendert FFmpeg im Backend mit Loudness-Normalisierung.
          </div>
        </div>
      </div>

      {/* Actions */}
      <div className="flex flex-wrap items-center gap-3 pt-2 border-t border-border">
        <button
          type="button"
          onClick={handleResetToDefault}
          className="text-xs font-semibold text-text-secondary hover:text-white inline-flex items-center gap-1.5"
        >
          <RotateCcw className="w-3.5 h-3.5" /> Auf Default zurücksetzen
        </button>
        <div className="ml-auto flex items-center gap-2">
          <button
            type="button"
            disabled={!isDirty || isSaving}
            onClick={handleReset}
            className="px-3 py-2 rounded-xl text-xs font-semibold text-text-secondary border border-border hover:text-white disabled:opacity-40"
          >
            {resetLabel}
          </button>
          <button
            type="button"
            disabled={!isDirty || isSaving}
            onClick={() => onSave(layout)}
            className="px-4 py-2 rounded-xl text-xs font-bold inline-flex items-center gap-2 bg-orange text-white shadow-[0_8px_22px_-8px_rgba(255,122,24,0.6)] hover:bg-orange-hover transition disabled:opacity-40 disabled:cursor-not-allowed"
          >
            <Save className="w-3.5 h-3.5" />
            {isSaving ? 'Speichert…' : saveLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
