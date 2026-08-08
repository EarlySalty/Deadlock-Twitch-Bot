import { useEffect, useMemo, useRef, useState } from 'react';
import type { CSSProperties, ReactNode } from 'react';
import { Camera, Layers, Maximize2, Save, RotateCcw, EyeOff, Eye } from 'lucide-react';
import type { LayoutBox, LayoutPayload, LayoutMode } from '@/types/socialMedia';
import { DEFAULT_LAYOUT, DEFAULT_SOURCE_HEIGHT, DEFAULT_SOURCE_WIDTH } from '@/types/socialMedia';
import type { DragMode } from '@/utils/socialMediaLayout';
import {
  TARGET_HEIGHT,
  TARGET_WIDTH,
  applyDrag,
  clampCamPositionToTarget,
  clampToFrame,
  formatBox,
  withBandHeight,
} from '@/utils/socialMediaLayout';

type BoxId = 'game_crop' | 'cam_crop' | 'cam_position';

type BoxColor = 'gold' | 'teal';

const BOX_COLORS: Record<BoxColor, { border: string; fill: string }> = {
  gold: { border: 'rgba(197, 160, 89, 0.95)', fill: 'rgba(197, 160, 89, 0.18)' },
  teal: { border: 'rgba(0, 217, 255, 0.95)', fill: 'rgba(0, 217, 255, 0.18)' },
};

const CORNER_HANDLES: DragMode[] = ['resize-tl', 'resize-tr', 'resize-bl', 'resize-br'];

interface FrameBox {
  id: BoxId;
  /** Anzeigerechteck im Koordinatensystem des Rahmens. */
  box: LayoutBox;
  label: string;
  color: BoxColor;
  /** `free` = verschieben und an den Ecken ziehen, `band` = nur die Unterkante. */
  interaction: 'free' | 'band';
}

interface DragState {
  pointerId: number;
  boxId: BoxId;
  mode: DragMode;
  startClient: { x: number; y: number };
  startBox: LayoutBox;
  scaleX: number;
  scaleY: number;
}

interface EditableFrameProps {
  frameWidth: number;
  frameHeight: number;
  boxes: FrameBox[];
  selectedBox: BoxId | null;
  onSelectBox: (id: BoxId) => void;
  onBoxChange: (id: BoxId, next: LayoutBox) => void;
  /** Kleines Label in der linken oberen Ecke. */
  caption: string;
  /** Hintergrundflächen (Game-Fläche, Muster), liegen unter den Boxen. */
  background?: ReactNode;
  style?: CSSProperties;
}

/**
 * Rahmen mit ziehbaren Rechtecken. Wird für beide Seiten benutzt: für den
 * Twitch-Quellframe (Crops) und für den Hochformat-Zielframe (Cam-Position).
 * Die Boxen kommen als Anzeigerechtecke herein; wie sie ins Layout
 * zurückgeschrieben werden, entscheidet der Aufrufer.
 */
function EditableFrame({
  frameWidth,
  frameHeight,
  boxes,
  selectedBox,
  onSelectBox,
  onBoxChange,
  caption,
  background,
  style,
}: EditableFrameProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const dragRef = useRef<DragState | null>(null);

  const handlePointerDown = (e: React.PointerEvent, boxId: BoxId, mode: DragMode) => {
    const container = containerRef.current;
    if (!container) return;
    const spec = boxes.find((b) => b.id === boxId);
    if (!spec) return;
    e.preventDefault();
    e.stopPropagation();
    onSelectBox(boxId);
    if (spec.interaction === 'band' && mode === 'move') return;
    const rect = container.getBoundingClientRect();
    dragRef.current = {
      pointerId: e.pointerId,
      boxId,
      mode,
      startClient: { x: e.clientX, y: e.clientY },
      startBox: { ...spec.box },
      scaleX: frameWidth / rect.width,
      scaleY: frameHeight / rect.height,
    };
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  };

  const handlePointerMove = (e: React.PointerEvent) => {
    const state = dragRef.current;
    if (!state || state.pointerId !== e.pointerId) return;
    const dx = (e.clientX - state.startClient.x) * state.scaleX;
    const dy = (e.clientY - state.startClient.y) * state.scaleY;
    const dragged = applyDrag(state.startBox, dx, dy, state.mode);
    onBoxChange(state.boxId, clampToFrame(dragged, frameWidth, frameHeight));
  };

  const handlePointerUp = (e: React.PointerEvent) => {
    if (dragRef.current?.pointerId === e.pointerId) dragRef.current = null;
  };

  return (
    <div
      ref={containerRef}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerUp}
      onPointerCancel={handlePointerUp}
      className="relative w-full select-none rounded-2xl overflow-hidden"
      style={{
        aspectRatio: `${frameWidth} / ${frameHeight}`,
        background: 'linear-gradient(135deg,#1F1815,#140D0A)',
        border: '1px solid var(--color-border)',
        ...style,
      }}
    >
      {background}

      <div className="absolute top-2 left-2 text-[10px] font-bold uppercase tracking-[0.16em] text-white/60 px-2 py-1 rounded-md bg-black/45 backdrop-blur-md z-30">
        {caption}
      </div>

      {boxes.map((spec) => {
        const { border, fill } = BOX_COLORS[spec.color];
        const isSelected = selectedBox === spec.id;
        const isBand = spec.interaction === 'band';
        return (
          <div
            key={spec.id}
            onPointerDown={(e) => handlePointerDown(e, spec.id, 'move')}
            className={`absolute select-none ${isBand ? '' : 'cursor-move'} ${
              isSelected ? 'z-20' : 'z-10'
            }`}
            style={{
              left: `${(spec.box.x / frameWidth) * 100}%`,
              top: `${(spec.box.y / frameHeight) * 100}%`,
              width: `${(spec.box.w / frameWidth) * 100}%`,
              height: `${(spec.box.h / frameHeight) * 100}%`,
              border: `2px solid ${border}`,
              background: fill,
              boxShadow: isSelected
                ? `0 0 0 3px ${border}, 0 8px 24px rgba(0,0,0,0.45)`
                : '0 4px 14px rgba(0,0,0,0.3)',
              backdropFilter: 'blur(2px)',
            }}
          >
            <div
              className="absolute top-1 left-1 text-[9px] font-bold uppercase tracking-[0.12em] text-white px-1.5 py-0.5 rounded"
              style={{ background: border }}
            >
              {spec.label}
            </div>
            <div className="absolute bottom-1 right-1 text-[9px] font-mono text-white/85 bg-black/55 px-1 py-0.5 rounded">
              {isBand ? `Höhe ${Math.round(spec.box.h)}` : formatBox(spec.box)}
            </div>

            {isBand ? (
              <div
                onPointerDown={(e) => handlePointerDown(e, spec.id, 'resize-b')}
                className="absolute left-0 right-0 h-3 cursor-ns-resize flex items-center justify-center"
                style={{ bottom: -6 }}
              >
                <div
                  style={{
                    width: 56,
                    height: 6,
                    background: border,
                    borderRadius: 3,
                    border: '2px solid #140D0A',
                  }}
                />
              </div>
            ) : (
              CORNER_HANDLES.map((mode) => {
                const isTop = mode === 'resize-tl' || mode === 'resize-tr';
                const isLeft = mode === 'resize-tl' || mode === 'resize-bl';
                const cursor =
                  mode === 'resize-tl' || mode === 'resize-br'
                    ? 'cursor-nwse-resize'
                    : 'cursor-nesw-resize';
                return (
                  <div
                    key={mode}
                    onPointerDown={(e) => handlePointerDown(e, spec.id, mode)}
                    className={`absolute w-3.5 h-3.5 ${cursor}`}
                    style={{
                      top: isTop ? -7 : 'auto',
                      bottom: !isTop ? -7 : 'auto',
                      left: isLeft ? -7 : 'auto',
                      right: !isLeft ? -7 : 'auto',
                      background: border,
                      borderRadius: 4,
                      border: '2px solid #140D0A',
                    }}
                  />
                );
              })
            )}
          </div>
        );
      })}
    </div>
  );
}

const GAME_PATTERN =
  'repeating-linear-gradient(45deg, rgba(197, 160, 89, 0.18) 0 14px, rgba(197, 160, 89, 0.06) 14px 28px)';
const SOURCE_PATTERN =
  'repeating-linear-gradient(45deg, rgba(255,255,255,0.04) 0 12px, transparent 12px 24px)';

interface PreviewProps {
  layout: LayoutPayload;
  camEnabled: boolean;
  mode: LayoutMode;
  selectedBox: BoxId | null;
  onSelectBox: (id: BoxId) => void;
  onBoxChange: (id: BoxId, next: LayoutBox) => void;
}

/** Twitch-Frame: was aus dem Bild ausgeschnitten wird. */
function SourcePreview({ layout, camEnabled, selectedBox, onSelectBox, onBoxChange }: PreviewProps) {
  const boxes: FrameBox[] = [
    { id: 'game_crop', box: layout.game_crop, label: 'Game-Ausschnitt', color: 'gold', interaction: 'free' },
  ];
  if (camEnabled) {
    boxes.push({
      id: 'cam_crop',
      box: layout.cam_crop,
      label: 'Cam-Ausschnitt',
      color: 'teal',
      interaction: 'free',
    });
  }

  return (
    <EditableFrame
      frameWidth={layout.source.width}
      frameHeight={layout.source.height}
      boxes={boxes}
      selectedBox={selectedBox}
      onSelectBox={onSelectBox}
      onBoxChange={onBoxChange}
      caption={`Twitch-Bild 16:9 · ${layout.source.width}×${layout.source.height}`}
      background={<div className="absolute inset-0" style={{ background: SOURCE_PATTERN }} />}
    />
  );
}

/** Hochformat-Frame: wo die Ausschnitte im fertigen Video landen. */
function TargetPreview({ layout, camEnabled, mode, selectedBox, onSelectBox, onBoxChange }: PreviewProps) {
  const bandHeight = layout.cam_position.h;
  const isStacked = mode === 'stacked';

  const boxes: FrameBox[] = [];
  if (camEnabled) {
    boxes.push({
      id: 'cam_position',
      // Im Streifen-Modus zählt nur die Höhe: der Streifen sitzt immer oben und
      // ist immer 1080 breit, genau wie im Renderer.
      box: isStacked
        ? { x: 0, y: 0, w: TARGET_WIDTH, h: bandHeight }
        : layout.cam_position,
      label: isStacked ? 'Cam-Streifen' : 'Cam-Kachel',
      color: 'teal',
      interaction: isStacked ? 'band' : 'free',
    });
  }

  const background =
    isStacked && camEnabled ? (
      <div
        className="absolute left-0 right-0 bottom-0 flex items-center justify-center"
        style={{ top: `${(bandHeight / TARGET_HEIGHT) * 100}%`, background: GAME_PATTERN }}
      >
        <span className="text-white/80 text-[10px] font-bold uppercase tracking-[0.16em]">Game</span>
      </div>
    ) : (
      <div className="absolute inset-0 flex items-center justify-center" style={{ background: GAME_PATTERN }}>
        <span className="text-white/80 text-[10px] font-bold uppercase tracking-[0.16em]">
          Game füllt das Bild
        </span>
      </div>
    );

  return (
    <EditableFrame
      frameWidth={TARGET_WIDTH}
      frameHeight={TARGET_HEIGHT}
      boxes={boxes}
      selectedBox={selectedBox}
      onSelectBox={onSelectBox}
      onBoxChange={onBoxChange}
      caption={`Hochformat · ${TARGET_WIDTH}×${TARGET_HEIGHT}`}
      background={background}
      style={{ borderColor: 'rgba(197, 160, 89, 0.24)' }}
    />
  );
}

interface LayoutEditorProps {
  initialLayout?: LayoutPayload;
  isSaving?: boolean;
  onSave: (layout: LayoutPayload) => void;
  onReset?: () => void;
  saveLabel?: string;
  resetLabel?: string;
}

/** Gespeicherte Layouts können ein cam_position aus dem alten Quellraum tragen. */
function normalizeLayout(payload: LayoutPayload): LayoutPayload {
  return { ...payload, cam_position: clampCamPositionToTarget(payload.cam_position) };
}

export function LayoutEditor({
  initialLayout,
  isSaving,
  onSave,
  onReset,
  saveLabel = 'Als Standard speichern',
  resetLabel = 'Zurücksetzen',
}: LayoutEditorProps) {
  const base = useMemo(() => normalizeLayout(initialLayout ?? DEFAULT_LAYOUT), [initialLayout]);
  const [layout, setLayout] = useState<LayoutPayload>(base);
  const camEnabled = layout.cam_enabled;
  const mode = layout.mode;
  const setCamEnabled = (next: boolean) => setLayout((l) => ({ ...l, cam_enabled: next }));
  const setMode = (next: LayoutMode) => setLayout((l) => ({ ...l, mode: next }));
  const [selectedBox, setSelectedBox] = useState<BoxId | null>('game_crop');

  // Sync wenn initialLayout wechselt (z.B. anderer Streamer).
  useEffect(() => {
    if (initialLayout) setLayout(normalizeLayout(initialLayout));
  }, [initialLayout]);

  const isDirty = useMemo(() => JSON.stringify(layout) !== JSON.stringify(base), [layout, base]);

  const handleBoxChange = (id: BoxId, next: LayoutBox) => {
    setLayout((l) => {
      if (id === 'cam_position' && l.mode === 'stacked') {
        // Nur die Streifenhöhe übernehmen, x/y/w des PiP-Rechtecks bleiben stehen.
        return { ...l, cam_position: withBandHeight(l.cam_position, next.h) };
      }
      return { ...l, [id]: next };
    });
  };

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
                mode === 'pip' ? 'bg-orange text-white shadow-[0_4px_14px_rgba(201, 168, 106, 0.35)]' : 'text-text-secondary hover:text-white'
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
                mode === 'stacked' ? 'bg-orange text-white shadow-[0_4px_14px_rgba(201, 168, 106, 0.35)]' : 'text-text-secondary hover:text-white'
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

      {/* Quelle + Ziel */}
      <div className="grid grid-cols-1 lg:grid-cols-[minmax(0,1fr)_auto] gap-6">
        {/* Quellframe */}
        <div className="space-y-3">
          <div>
            <div className="text-xs text-text-secondary uppercase tracking-[0.16em] font-bold">
              Quelle · Twitch-Bild
            </div>
            <div className="text-[11px] text-text-secondary">
              Was aus dem Twitch-Bild ausgeschnitten wird.
            </div>
          </div>
          <SourcePreview
            layout={layout}
            camEnabled={camEnabled}
            mode={mode}
            selectedBox={selectedBox}
            onSelectBox={setSelectedBox}
            onBoxChange={handleBoxChange}
          />
          <div className="grid grid-cols-2 gap-2 text-[11px]">
            <button
              type="button"
              onClick={() => setSelectedBox('game_crop')}
              className={`px-2.5 py-2 rounded-lg border font-semibold uppercase tracking-[0.14em] ${
                selectedBox === 'game_crop'
                  ? 'border-orange/70 text-orange bg-orange/10'
                  : 'border-border text-text-secondary hover:text-white'
              }`}
            >
              Game-Ausschnitt
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
              <span className="inline-flex items-center justify-center gap-1.5">
                <Camera className="w-3 h-3" /> Cam-Ausschnitt
              </span>
            </button>
          </div>
        </div>

        {/* Zielframe */}
        <div className="space-y-3 lg:w-[340px]">
          <div>
            <div className="text-xs text-text-secondary uppercase tracking-[0.16em] font-bold">
              Ziel · Hochformat 9:16
            </div>
            <div className="text-[11px] text-text-secondary">
              Wo der Cam-Ausschnitt im fertigen Video landet.
            </div>
          </div>
          <div className="mx-auto" style={{ maxWidth: 320 }}>
            <TargetPreview
              layout={layout}
              camEnabled={camEnabled}
              mode={mode}
              selectedBox={selectedBox}
              onSelectBox={setSelectedBox}
              onBoxChange={handleBoxChange}
            />
          </div>
          <div className="text-[11px] text-text-secondary leading-relaxed">
            {!camEnabled
              ? 'Cam ist aus: das Game füllt das ganze Bild.'
              : mode === 'pip'
              ? `Cam-Kachel frei ziehen und an den Ecken skalieren: ${formatBox(layout.cam_position)}.`
              : `Cam-Streifen oben, Höhe an der Unterkante ziehen: ${Math.round(layout.cam_position.h)} von ${TARGET_HEIGHT} px.`}
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
            className="px-4 py-2 rounded-xl text-xs font-bold inline-flex items-center gap-2 bg-orange text-white shadow-[0_8px_22px_-8px_rgba(201, 168, 106, 0.6)] hover:bg-orange-hover transition disabled:opacity-40 disabled:cursor-not-allowed"
          >
            <Save className="w-3.5 h-3.5" />
            {isSaving ? 'Speichert…' : saveLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
