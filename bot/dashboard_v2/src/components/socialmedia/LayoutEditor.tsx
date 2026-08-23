import { useEffect, useMemo, useRef, useState } from 'react';
import type { CSSProperties, ReactNode } from 'react';
import { Camera, Image as ImageIcon, Layers, Maximize2, Save, RotateCcw, EyeOff, Eye } from 'lucide-react';
import { useT } from '@/context/LanguageContext';
import type { LayoutBox, LayoutPayload, LayoutMode } from '@/types/socialMedia';
import { DEFAULT_LAYOUT, DEFAULT_SOURCE_HEIGHT, DEFAULT_SOURCE_WIDTH } from '@/types/socialMedia';
import type { DragMode } from '@/utils/socialMediaLayout';
import {
  MAX_BAND_HEIGHT,
  TARGET_HEIGHT,
  TARGET_WIDTH,
  applyDrag,
  ausschnittRahmen,
  cappedTileWidth,
  clampCamPositionToTarget,
  clampToFrame,
  formatBox,
  normalizeStoredCamPosition,
  withBandHeight,
} from '@/utils/socialMediaLayout';

type BoxId = 'game_crop' | 'cam_crop' | 'cam_position';

type BoxColor = 'gold' | 'messing';

// Zwei Metalle statt Gold gegen Neonblau: das Blau war ein Farbbruch im
// Industrial-Gold-Theme. Weil Antik-Gold und helles Messing farblich nah
// beieinander liegen, traegt die Linienart die Unterscheidung, nicht der Ton.
const BOX_COLORS: Record<BoxColor, { border: string; fill: string; dashed: boolean }> = {
  gold: { border: 'rgba(197, 160, 89, 0.95)', fill: 'rgba(197, 160, 89, 0.16)', dashed: false },
  messing: { border: 'rgba(241, 210, 153, 0.95)', fill: 'rgba(241, 210, 153, 0.14)', dashed: true },
};

/** Tinte auf Metallflaechen. Weiss auf Gold liegt bei ~1.8:1 und ist unlesbar. */
const AUF_METALL = '#241A12';

const CORNER_HANDLES: DragMode[] = ['resize-tl', 'resize-tr', 'resize-bl', 'resize-br'];

interface FrameBox {
  id: BoxId;
  /** Anzeigerechteck im Koordinatensystem des Rahmens. */
  box: LayoutBox;
  label: string;
  color: BoxColor;
  /** `free` = verschieben und an den Ecken ziehen, `band` = nur die Unterkante. */
  interaction: 'free' | 'band';
  /** Liegt in der Box unter Label und Griffen, z.B. der Bildausschnitt. */
  content?: ReactNode;
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
  const t = useT();
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
        const { border, fill, dashed } = BOX_COLORS[spec.color];
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
              border: `2px ${dashed ? 'dashed' : 'solid'} ${border}`,
              background: fill,
              boxShadow: isSelected
                ? `0 0 0 3px ${border}, 0 8px 24px rgba(0,0,0,0.45)`
                : '0 4px 14px rgba(0,0,0,0.3)',
              backdropFilter: 'blur(2px)',
            }}
          >
            {spec.content}
            <div
              className="absolute top-1 left-1 text-[9px] font-bold uppercase tracking-[0.12em] px-1.5 py-0.5 rounded z-10"
              style={{ background: border, color: AUF_METALL }}
            >
              {spec.label}
            </div>
            <div className="absolute bottom-1 right-1 text-[9px] font-mono text-white/85 bg-black/55 px-1 py-0.5 rounded">
              {isBand ? t('Höhe {height}', { height: Math.round(spec.box.h) }) : formatBox(spec.box)}
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

/** Ein Clip, dessen Standbild als Vorschau im Editor liegen kann. */
export interface VorschauClip {
  id: string;
  titel: string;
  /** Standbild des Clips, 16:9. */
  bildUrl: string;
}

/**
 * Twitch legt dasselbe Standbild in mehreren Groessen ab. Die Liste liefert die
 * kleine Variante; im Editor steht das Bild aber auf halber Bildschirmbreite und
 * sieht in 480 Pixeln matschig aus. Die grosse Variante hat zusaetzlich exakt
 * 16:9, waehrend 480x272 leicht daneben liegt.
 */
export function grossesStandbild(url: string): string {
  return url.replace(/-\d+x\d+\.(jpg|jpeg|png)$/i, '-1920x1080.$1');
}

/**
 * Stellt einen Ausschnitt aus dem Quellbild genau so dar, wie der Renderer ihn
 * baut: `crop`, dann `scale` mit `force_original_aspect_ratio=increase` und
 * mittigem Nachschnitt auf die Zielgroesse (siehe `build_compose_filter` in
 * rust/crates/tb-social-media/src/video_processor.rs). Das ist rechnerisch
 * `object-fit: cover` auf dem Ausschnitt, deshalb genuegen zwei verschachtelte
 * Kaesten statt eines Canvas.
 */
function AusschnittBild({
  bildUrl,
  quelle,
  crop,
  zielBreite,
  zielHoehe,
}: {
  bildUrl: string;
  quelle: { width: number; height: number };
  crop: LayoutBox;
  zielBreite: number;
  zielHoehe: number;
}) {
  const rahmen = ausschnittRahmen(quelle, crop, zielBreite, zielHoehe);
  if (!rahmen) return null;
  return (
    <div className="absolute inset-0 overflow-hidden">
      <img
        src={bildUrl}
        alt=""
        draggable={false}
        className="absolute max-w-none pointer-events-none"
        style={{
          width: `${rahmen.breite}%`,
          height: `${rahmen.hoehe}%`,
          left: `${rahmen.links}%`,
          top: `${rahmen.oben}%`,
        }}
      />
    </div>
  );
}

interface PreviewProps {
  layout: LayoutPayload;
  camEnabled: boolean;
  mode: LayoutMode;
  selectedBox: BoxId | null;
  onSelectBox: (id: BoxId) => void;
  onBoxChange: (id: BoxId, next: LayoutBox) => void;
  /** Standbild des gewaehlten Clips; ohne das faellt die Vorschau aufs Muster. */
  bildUrl?: string | null;
}

/** Twitch-Frame: was aus dem Bild ausgeschnitten wird. */
function SourcePreview({ layout, camEnabled, selectedBox, onSelectBox, onBoxChange, bildUrl }: PreviewProps) {
  const t = useT();
  const boxes: FrameBox[] = [
    { id: 'game_crop', box: layout.game_crop, label: t('Game-Ausschnitt'), color: 'gold', interaction: 'free' },
  ];
  if (camEnabled) {
    boxes.push({
      id: 'cam_crop',
      box: layout.cam_crop,
      label: t('Cam-Ausschnitt'),
      color: 'messing',
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
      caption={t('Twitch-Bild 16:9 · {width}×{height}', {
        width: layout.source.width,
        height: layout.source.height,
      })}
      background={
        bildUrl ? (
          <img
            src={bildUrl}
            alt=""
            draggable={false}
            className="absolute inset-0 h-full w-full object-cover pointer-events-none"
          />
        ) : (
          <div className="absolute inset-0" style={{ background: SOURCE_PATTERN }} />
        )
      }
    />
  );
}

/** Hochformat-Frame: wo die Ausschnitte im fertigen Video landen. */
function TargetPreview({ layout, camEnabled, mode, selectedBox, onSelectBox, onBoxChange, bildUrl }: PreviewProps) {
  const t = useT();
  const bandHeight = layout.cam_position.h;
  const isStacked = mode === 'stacked';

  const boxes: FrameBox[] = [];
  if (camEnabled) {
    const camBox = isStacked
      ? { x: 0, y: 0, w: TARGET_WIDTH, h: bandHeight }
      : layout.cam_position;
    boxes.push({
      id: 'cam_position',
      // Im Streifen-Modus zählt nur die Höhe: der Streifen sitzt immer oben und
      // ist immer 1080 breit, genau wie im Renderer.
      box: camBox,
      label: isStacked ? t('Cam-Streifen') : t('Cam-Kachel'),
      color: 'messing',
      interaction: isStacked ? 'band' : 'free',
      content: bildUrl ? (
        <AusschnittBild
          bildUrl={bildUrl}
          quelle={layout.source}
          crop={layout.cam_crop}
          zielBreite={camBox.w}
          zielHoehe={camBox.h}
        />
      ) : undefined,
    });
  }

  // Im Streifen-Modus rechnet der Renderer die Game-Flaeche auf die Resthoehe
  // unter dem Streifen, im PiP-Modus auf den vollen Frame. Beides muss die
  // Vorschau nachbilden, sonst zeigt sie einen anderen Ausschnitt als das Video.
  const gameHoehe = isStacked && camEnabled ? TARGET_HEIGHT - bandHeight : TARGET_HEIGHT;
  const gameOben = isStacked && camEnabled ? (bandHeight / TARGET_HEIGHT) * 100 : 0;

  const gameFlaeche = bildUrl ? (
    <AusschnittBild
      bildUrl={bildUrl}
      quelle={layout.source}
      crop={layout.game_crop}
      zielBreite={TARGET_WIDTH}
      zielHoehe={gameHoehe}
    />
  ) : (
    <div className="absolute inset-0 flex items-center justify-center" style={{ background: GAME_PATTERN }}>
      <span className="text-white/80 text-[10px] font-bold uppercase tracking-[0.16em]">
        {isStacked && camEnabled ? t('Game') : t('Game füllt das Bild')}
      </span>
    </div>
  );

  const background = (
    <div className="absolute left-0 right-0 bottom-0 overflow-hidden" style={{ top: `${gameOben}%` }}>
      {gameFlaeche}
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
      caption={t('Hochformat · {width}×{height}', { width: TARGET_WIDTH, height: TARGET_HEIGHT })}
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
  /**
   * Echte Clips als Vorschau. Ohne die Liste zeigt der Editor wie bisher nur
   * Muster; an einem Muster laesst sich ein Ausschnitt aber nicht beurteilen.
   */
  vorschauClips?: VorschauClip[];
}

/** Gespeicherte Layouts können ein cam_position aus dem alten Quellraum tragen. */
function normalizeLayout(payload: LayoutPayload): LayoutPayload {
  return {
    ...payload,
    cam_position: normalizeStoredCamPosition(payload.cam_position, payload.mode),
  };
}

export function LayoutEditor({
  initialLayout,
  isSaving,
  onSave,
  onReset,
  saveLabel,
  resetLabel,
  vorschauClips = [],
}: LayoutEditorProps) {
  const t = useT();
  const base = useMemo(() => normalizeLayout(initialLayout ?? DEFAULT_LAYOUT), [initialLayout]);
  const [layout, setLayout] = useState<LayoutPayload>(base);
  const camEnabled = layout.cam_enabled;
  const mode = layout.mode;
  const setCamEnabled = (next: boolean) => setLayout((l) => ({ ...l, cam_enabled: next }));
  const setMode = (next: LayoutMode) => setLayout((l) => ({ ...l, mode: next }));
  const [selectedBox, setSelectedBox] = useState<BoxId | null>('game_crop');

  // Vorschaubild: der erste Clip ist vorgewaehlt, damit der Editor nicht leer
  // aufgeht. `ohneBild` ist die bewusste Abwahl zurueck aufs Muster.
  const [vorschauId, setVorschauId] = useState<string | null>(null);
  const [ohneBild, setOhneBild] = useState(false);
  const [grossFehlt, setGrossFehlt] = useState(false);
  const gewaehlterClip = ohneBild
    ? null
    : vorschauClips.find((clip) => clip.id === vorschauId) ?? vorschauClips[0] ?? null;

  // Die grosse Variante liegt nicht bei jedem Clip. Ein stiller Vorablauf klaert
  // das, bevor der Rahmen sie anzeigt; faellt sie aus, bleibt das kleine Bild.
  useEffect(() => {
    setGrossFehlt(false);
    if (!gewaehlterClip) return;
    const gross = grossesStandbild(gewaehlterClip.bildUrl);
    if (gross === gewaehlterClip.bildUrl) return;
    const probe = new Image();
    probe.onerror = () => setGrossFehlt(true);
    probe.src = gross;
    return () => {
      probe.onerror = null;
    };
  }, [gewaehlterClip?.id, gewaehlterClip?.bildUrl]);

  const bildUrl = gewaehlterClip
    ? grossFehlt
      ? gewaehlterClip.bildUrl
      : grossesStandbild(gewaehlterClip.bildUrl)
    : null;

  // Sync wenn initialLayout wechselt (z.B. anderer Streamer).
  useEffect(() => {
    if (initialLayout) setLayout(normalizeLayout(initialLayout));
  }, [initialLayout]);

  const isDirty = useMemo(() => JSON.stringify(layout) !== JSON.stringify(base), [layout, base]);

  const handleBoxChange = (id: BoxId, next: LayoutBox) => {
    setLayout((l) => {
      if (id === 'cam_position') {
        // Streifen-Modus: nur die Höhe übernehmen, x und w des PiP-Rechtecks
        // bleiben stehen. PiP: gerade Kantenlängen wie im Renderer, Breite
        // knapp unter Framebreite, sonst liest der Legacy-Erkenner die Kachel
        // beim nächsten Laden als Altlast und setzt sie auf den Standard.
        return l.mode === 'stacked'
          ? { ...l, cam_position: withBandHeight(l.cam_position, next.h) }
          : { ...l, cam_position: clampCamPositionToTarget(cappedTileWidth(next)) };
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
          <Maximize2 className="w-4 h-4 text-orange" /> {t('Layout-Editor')}
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
                <Layers className="w-3.5 h-3.5" /> {t('PiP')}
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
                <Layers className="w-3.5 h-3.5 rotate-90" /> {t('Stacked')}
              </span>
            </button>
          </div>

          {/* Cam toggle */}
          <button
            type="button"
            onClick={() => setCamEnabled(!camEnabled)}
            className={`inline-flex items-center gap-2 px-3 py-2 rounded-xl text-xs font-semibold border transition ${
              camEnabled
                ? 'bg-accent/15 text-accent border-accent/40'
                : 'bg-bg/60 text-text-secondary border-border hover:text-white'
            }`}
          >
            {camEnabled ? <Eye className="w-3.5 h-3.5" /> : <EyeOff className="w-3.5 h-3.5" />}
            {camEnabled ? t('Cam an') : t('Cam aus')}
          </button>
        </div>
      </div>

      {/* Vorschauclip: an einem echten Bild sieht man den Ausschnitt, am Muster nicht. */}
      {vorschauClips.length > 0 && (
        <div className="flex flex-wrap items-center gap-2 rounded-xl border border-border bg-bg/40 px-3 py-2">
          <span className="inline-flex items-center gap-1.5 text-[11px] font-bold uppercase tracking-[0.14em] text-text-secondary">
            <ImageIcon className="w-3.5 h-3.5 text-accent" /> {t('Vorschau-Clip')}
          </span>
          <select
            value={ohneBild ? '' : gewaehlterClip?.id ?? ''}
            onChange={(event) => {
              const wert = event.target.value;
              setOhneBild(wert === '');
              setVorschauId(wert === '' ? null : wert);
            }}
            className="min-w-0 flex-1 rounded-lg border border-border bg-background/80 px-2.5 py-1.5 text-xs font-medium text-white outline-none transition-colors focus:border-border-hover"
          >
            {vorschauClips.map((clip) => (
              <option key={clip.id} value={clip.id}>
                {clip.titel}
              </option>
            ))}
            <option value="">{t('Ohne Bild (Muster)')}</option>
          </select>
          <span className="text-[11px] text-text-secondary">
            {t('Der Ausschnitt gilt danach für alle Clips dieses Kanals.')}
          </span>
        </div>
      )}

      {/* Quelle + Ziel */}
      <div className="grid grid-cols-1 lg:grid-cols-[minmax(0,1fr)_auto] gap-6">
        {/* Quellframe */}
        <div className="space-y-3">
          <div>
            <div className="text-xs text-text-secondary uppercase tracking-[0.16em] font-bold">
              {t('Quelle · Twitch-Bild')}
            </div>
            <div className="text-[11px] text-text-secondary">
              {t('Was aus dem Twitch-Bild ausgeschnitten wird.')}
            </div>
          </div>
          <SourcePreview
            layout={layout}
            camEnabled={camEnabled}
            mode={mode}
            selectedBox={selectedBox}
            onSelectBox={setSelectedBox}
            onBoxChange={handleBoxChange}
            bildUrl={bildUrl}
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
              {t('Game-Ausschnitt')}
            </button>
            <button
              type="button"
              disabled={!camEnabled}
              onClick={() => setSelectedBox('cam_crop')}
              className={`px-2.5 py-2 rounded-lg border font-semibold uppercase tracking-[0.14em] ${
                selectedBox === 'cam_crop'
                  ? 'border-accent/70 text-accent bg-accent/10'
                  : 'border-border text-text-secondary hover:text-white'
              } ${!camEnabled ? 'opacity-40 cursor-not-allowed' : ''}`}
            >
              <span className="inline-flex items-center justify-center gap-1.5">
                <Camera className="w-3 h-3" /> {t('Cam-Ausschnitt')}
              </span>
            </button>
          </div>
        </div>

        {/* Zielframe */}
        <div className="space-y-3 lg:w-[340px]">
          <div>
            <div className="text-xs text-text-secondary uppercase tracking-[0.16em] font-bold">
              {t('Ziel · Hochformat 9:16')}
            </div>
            <div className="text-[11px] text-text-secondary">
              {t('Wo der Cam-Ausschnitt im fertigen Video landet.')}
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
              bildUrl={bildUrl}
            />
          </div>
          <div className="text-[11px] text-text-secondary leading-relaxed">
            {!camEnabled
              ? t('Cam ist aus: das Game füllt das ganze Bild.')
              : mode === 'pip'
              ? t('Cam-Kachel frei ziehen und an den Ecken skalieren: {box}.', {
                  box: formatBox(layout.cam_position),
                })
              : t('Cam-Streifen oben, Höhe an der Unterkante ziehen: {height} von maximal {max} px.', {
                  height: Math.round(layout.cam_position.h),
                  max: MAX_BAND_HEIGHT,
                })}
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
          <RotateCcw className="w-3.5 h-3.5" /> {t('Auf Default zurücksetzen')}
        </button>
        <div className="ml-auto flex items-center gap-2">
          <button
            type="button"
            disabled={!isDirty || isSaving}
            onClick={handleReset}
            className="px-3 py-2 rounded-xl text-xs font-semibold text-text-secondary border border-border hover:text-white disabled:opacity-40"
          >
            {resetLabel ?? t('Zurücksetzen')}
          </button>
          <button
            type="button"
            disabled={!isDirty || isSaving}
            onClick={() => onSave(layout)}
            className="px-4 py-2 rounded-xl text-xs font-bold inline-flex items-center gap-2 bg-orange text-white shadow-[0_8px_22px_-8px_rgba(201, 168, 106, 0.6)] hover:bg-orange-hover transition disabled:opacity-40 disabled:cursor-not-allowed"
          >
            <Save className="w-3.5 h-3.5" />
            {isSaving ? t('Speichert…') : (saveLabel ?? t('Als Standard speichern'))}
          </button>
        </div>
      </div>
    </div>
  );
}
