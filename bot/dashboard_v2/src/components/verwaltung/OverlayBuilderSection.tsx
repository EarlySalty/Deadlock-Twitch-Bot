import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties } from 'react';
import { flushSync } from 'react-dom';
import { motion } from 'framer-motion';
import { Check, Copy, Maximize, Minimize, RectangleHorizontal, X } from 'lucide-react';
import { OverlayCanvasGuides } from './OverlayCanvasGuides';
import { moveSelection, selectionBounds, selectionIntersects, toggleSelection, type SnapHit } from './overlaySelection';

type EditorMode = 'simple' | 'advanced';
type OverlayTheme = 'dark' | 'light' | 'accent';
type OverlayLayout = 'box' | 'bar' | 'canvas';
type OverlayMode = 'all' | 'standard' | 'ranked' | 'brawl';
type ModuleKey =
  | 'header'
  | 'rank'
  | 'winrate'
  | 'today'
  | 'streak'
  | 'kd'
  | 'lastmatch'
  | 'mostplayed'
  | 'recent'
  | 'live'
  | 'branding';

const THEMES: Array<{ value: OverlayTheme; label: string }> = [
  { value: 'dark', label: 'Nachtgold' },
  { value: 'light', label: 'Hell' },
  { value: 'accent', label: 'Akzent' },
];

const LAYOUTS: Array<{ value: OverlayLayout; label: string }> = [
  { value: 'canvas', label: 'Module frei bearbeiten' },
  { value: 'box', label: 'Feste Karte' },
  { value: 'bar', label: 'Feste Leiste' },
];

const MODES: Array<{ value: OverlayMode; label: string }> = [
  { value: 'all', label: 'Alle Modi' },
  { value: 'standard', label: 'Standard' },
  { value: 'ranked', label: 'Ranked' },
  { value: 'brawl', label: 'Street Brawl' },
];

const MODULES: Array<{ key: ModuleKey; label: string }> = [
  { key: 'header', label: 'Spielername & Live-Badge' },
  { key: 'rank', label: 'Rang & Abzeichen' },
  { key: 'winrate', label: 'Siegquote (letzte Spiele)' },
  { key: 'today', label: 'Heute (Siege/Niederlagen)' },
  { key: 'streak', label: 'Aktuelle Serie' },
  { key: 'kd', label: 'K/D' },
  { key: 'lastmatch', label: 'Letztes Match' },
  { key: 'mostplayed', label: 'Meistgespielter Held' },
  { key: 'recent', label: 'Match-Verlauf' },
  { key: 'live', label: 'Laufendes Spiel (Held & Minute)' },
  { key: 'branding', label: 'Community-Logo' },
];

const DEFAULT_MODULES: Record<ModuleKey, boolean> = {
  header: true,
  rank: true,
  winrate: true,
  today: true,
  streak: true,
  kd: true,
  lastmatch: false,
  mostplayed: false,
  recent: true,
  live: true,
  branding: true,
};

type OverlaySource = { x: number; y: number; width: number; height: number };
type SourceField = keyof OverlaySource;
type SourceDraft = Record<SourceField, string>;
type SourceDrafts = Record<ModuleKey, SourceDraft>;

const sourceDraft = (source: OverlaySource): SourceDraft => ({
  x: String(Math.round(source.x)),
  y: String(Math.round(source.y)),
  width: String(Math.round(source.width)),
  height: String(Math.round(source.height)),
});

type SavedOverlayConfig = {
  editorVersion?: number;
  editorMode?: EditorMode;
  layout?: OverlayLayout;
  theme?: OverlayTheme;
  mode?: OverlayMode;
  opacity?: number;
  recentN?: number;
  modules?: Partial<Record<ModuleKey, boolean>>;
  accent?: string;
  background?: string;
  text?: string;
  radius?: number;
  canvasWidth?: number;
  canvasHeight?: number;
  sources?: Partial<Record<ModuleKey, OverlaySource>>;
};

const DEFAULT_CANVAS_WIDTH = 1920;
const DEFAULT_CANVAS_HEIGHT = 1080;

const DEFAULT_CANVAS_SOURCES: Record<ModuleKey, OverlaySource> = {
  header: { x: 48, y: 48, width: 420, height: 84 },
  live: { x: 492, y: 48, width: 220, height: 84 },
  rank: { x: 48, y: 156, width: 664, height: 170 },
  winrate: { x: 48, y: 350, width: 320, height: 112 },
  today: { x: 392, y: 350, width: 320, height: 112 },
  streak: { x: 48, y: 486, width: 320, height: 112 },
  kd: { x: 392, y: 486, width: 320, height: 112 },
  lastmatch: { x: 48, y: 622, width: 320, height: 112 },
  mostplayed: { x: 392, y: 622, width: 320, height: 112 },
  recent: { x: 48, y: 758, width: 664, height: 112 },
  branding: { x: 48, y: 894, width: 664, height: 80 },
};

const CANVAS_PRESETS = [
  { label: 'Full HD', width: 1920, height: 1080 },
  { label: 'HD', width: 1280, height: 720 },
  { label: 'Quadrat', width: 1080, height: 1080 },
] as const;

type DragState = {
  pointerId: number;
  target: HTMLElement;
  key: ModuleKey;
  mode: 'move' | 'resize';
  startX: number;
  startY: number;
  source: OverlaySource;
  members: Array<{ key: ModuleKey; source: OverlaySource }>;
  targets: OverlaySource[];
};

type MarqueeState = {
  pointerId: number;
  target: HTMLElement;
  startX: number;
  startY: number;
  initial: ModuleKey[];
};

function clampSource(source: OverlaySource, canvasWidth: number, canvasHeight: number): OverlaySource {
  const finite = (value: number, fallback: number) => typeof value === 'number' && Number.isFinite(value) ? Math.round(value) : fallback;
  const width = Math.min(canvasWidth, Math.max(120, finite(source.width, 120)));
  const height = Math.min(canvasHeight, Math.max(48, finite(source.height, 48)));
  return {
    x: Math.min(Math.max(0, finite(source.x, 0)), Math.max(0, canvasWidth - width)),
    y: Math.min(Math.max(0, finite(source.y, 0)), Math.max(0, canvasHeight - height)),
    width,
    height,
  };
}

// Beim Ziehen der rechten unteren Ecke bleibt die linke obere Ecke fest.
function resizeSource(source: OverlaySource, width: number, height: number, canvasWidth: number, canvasHeight: number): OverlaySource {
  return clampSource({
    ...source,
    width: Math.min(canvasWidth - source.x, width),
    height: Math.min(canvasHeight - source.y, height),
  }, canvasWidth, canvasHeight);
}

const CHECKER_STYLE: CSSProperties = {
  backgroundColor: '#101114',
  backgroundImage:
    'linear-gradient(45deg, rgba(255,255,255,0.08) 25%, transparent 25%), linear-gradient(-45deg, rgba(255,255,255,0.08) 25%, transparent 25%), linear-gradient(45deg, transparent 75%, rgba(255,255,255,0.08) 75%), linear-gradient(-45deg, transparent 75%, rgba(255,255,255,0.08) 75%)',
  backgroundPosition: '0 0, 0 10px, 10px -10px, -10px 0',
  backgroundSize: '20px 20px',
};

function useDebouncedValue<T>(value: T, delayMs: number): T {
  const [debouncedValue, setDebouncedValue] = useState(value);

  useEffect(() => {
    const timeoutId = window.setTimeout(() => {
      setDebouncedValue(value);
    }, delayMs);

    return () => window.clearTimeout(timeoutId);
  }, [delayMs, value]);

  return debouncedValue;
}

type OverlayBuilderSectionProps = {
  login: string;
};

export function OverlayBuilderSection({ login }: OverlayBuilderSectionProps) {
  const normalizedLogin = login.trim();
  const [theme, setTheme] = useState<OverlayTheme>('dark');
  const [layout, setLayout] = useState<OverlayLayout>('canvas');
  const [editorMode, setEditorMode] = useState<EditorMode>('advanced');
  const [mode, setMode] = useState<OverlayMode>('all');
  const [opacity, setOpacity] = useState<number>(85);
  const [recentN, setRecentN] = useState<number>(10);
  const [modules, setModules] = useState<Record<ModuleKey, boolean>>(DEFAULT_MODULES);
  const [accent, setAccent] = useState('#d6b56c');
  const [background, setBackground] = useState('#0d0f14');
  const [text, setText] = useState('#f4f7fb');
  const [radius, setRadius] = useState(18);
  const [canvasWidth, setCanvasWidth] = useState(DEFAULT_CANVAS_WIDTH);
  const [canvasHeight, setCanvasHeight] = useState(DEFAULT_CANVAS_HEIGHT);
  const [canvasWidthDraft, setCanvasWidthDraft] = useState(String(DEFAULT_CANVAS_WIDTH));
  const [canvasHeightDraft, setCanvasHeightDraft] = useState(String(DEFAULT_CANVAS_HEIGHT));
  const [sources, setSources] = useState<Record<ModuleKey, OverlaySource>>(DEFAULT_CANVAS_SOURCES);
  const [sourceDrafts, setSourceDrafts] = useState<SourceDrafts>(() => {
    const next = {} as SourceDrafts;
    for (const key of Object.keys(DEFAULT_CANVAS_SOURCES) as ModuleKey[]) next[key] = sourceDraft(DEFAULT_CANVAS_SOURCES[key]);
    return next;
  });
  const [editingField, setEditingField] = useState<{ key: ModuleKey; field: SourceField } | null>(null);
  const [editingCanvasDimension, setEditingCanvasDimension] = useState<'width' | 'height' | null>(null);
  const [selectedSource, setSelectedSource] = useState<ModuleKey>('rank');
  const [selectedSources, setSelectedSources] = useState<ModuleKey[]>(['rank']);
  const visibleSelection = selectedSources.filter(key => modules[key]);
  const hasSelection = visibleSelection.length > 0;
  const selectSource = (key: ModuleKey, additive = false) => {
    if (!modules[key]) return;
    const next = additive ? toggleSelection(visibleSelection, key) : [key];
    setSelectedSource(next.includes(key) ? key : next[next.length - 1] || key);
    setSelectedSources(next);
  };
  const [magnetEnabled, setMagnetEnabled] = useState(true);
  const magnetRef = useRef(true);
  const [snapHits, setSnapHits] = useState<SnapHit[]>([]);
  const [marquee, setMarquee] = useState<OverlaySource | null>(null);
  const marqueeRef = useRef<MarqueeState | null>(null);
  const sceneRef = useRef({ sources, modules });
  useLayoutEffect(() => { sceneRef.current = { sources, modules }; }, [sources, modules]);
  const [activeSource, setActiveSource] = useState<ModuleKey | null>(null);
  const [touchSource, setTouchSource] = useState<ModuleKey | null>(null);
  const [previewHostWidth, setPreviewHostWidth] = useState(560);
  const [previewHostHeight, setPreviewHostHeight] = useState(520);
  const [outputHostWidth, setOutputHostWidth] = useState(560);
  const [theaterMode, setTheaterMode] = useState(false);
  const [fullscreen, setFullscreen] = useState(false);
  const [fullscreenNotice, setFullscreenNotice] = useState('');
  const [previewBackdrop, setPreviewBackdrop] = useState<'checker' | 'dark' | 'light'>('dark');
  const [copied, setCopied] = useState(false);
  const [saved, setSaved] = useState(false);
  const [saveFailed, setSaveFailed] = useState(false);
  const [retainedUrl, setRetainedUrl] = useState<string | null>(null);
  const [copyFailed, setCopyFailed] = useState(false);
  const editorRef = useRef<HTMLDivElement>(null);
  const previewHostRef = useRef<HTMLDivElement>(null);
  const outputHostRef = useRef<HTMLDivElement>(null);
  const previewPanelRef = useRef<HTMLDivElement>(null);
  const fullscreenPendingRef = useRef(false);
  const fullscreenExitRef = useRef(0);
  const dragRef = useRef<DragState | null>(null);
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const outputIframeRef = useRef<HTMLIFrameElement>(null);
  const expandedPreview = theaterMode || fullscreen;

  const endGesture = useCallback(() => {
    const active = dragRef.current || marqueeRef.current;
    dragRef.current = null;
    marqueeRef.current = null;
    setMarquee(null);
    setSnapHits([]);
    setActiveSource(null);
    if (active?.target.hasPointerCapture(active.pointerId)) active.target.releasePointerCapture(active.pointerId);
  }, []);

  const toggleFullscreen = async () => {
    const panel = previewPanelRef.current;
    if (!panel || fullscreenPendingRef.current) return;
    endGesture();
    fullscreenPendingRef.current = true;
    setFullscreenNotice('');
    try {
      if (document.fullscreenElement === panel) {
        await document.exitFullscreen();
      } else {
        panel.focus({ preventScroll: true });
        if (!panel.requestFullscreen || !document.fullscreenEnabled) {
          setTheaterMode(true);
          setFullscreenNotice('Vollbild ist hier nicht verfügbar. Der Kino-Modus ist geöffnet.');
          return;
        }
        await panel.requestFullscreen();
      }
    } catch {
      if (document.fullscreenElement === panel) {
        setFullscreenNotice('Vollbild konnte nicht beendet werden. Drücke Escape.');
      } else {
        setTheaterMode(true);
        setFullscreenNotice('Vollbild wurde nicht zugelassen. Der Kino-Modus ist geöffnet.');
      }
    } finally {
      fullscreenPendingRef.current = false;
    }
  };

  useEffect(() => {
    const panel = previewPanelRef.current;
    if (!panel) return;
    let wasFullscreen = document.fullscreenElement === panel;
    const updateFullscreen = () => {
      endGesture();
      const active = document.fullscreenElement === panel;
      setFullscreen(active);
      if (wasFullscreen && !active) {
        fullscreenExitRef.current = performance.now();
        panel.focus({ preventScroll: true });
      }
      wasFullscreen = active;
    };
    document.addEventListener('fullscreenchange', updateFullscreen);
    return () => document.removeEventListener('fullscreenchange', updateFullscreen);
  }, [endGesture, normalizedLogin]);

  const syncPreview = () => {
    iframeRef.current?.contentWindow?.postMessage({ type: 'ddc-overlay-scene', sources }, window.location.origin);
    outputIframeRef.current?.contentWindow?.postMessage({ type: 'ddc-overlay-scene', sources }, window.location.origin);
  };

  useEffect(() => {
    iframeRef.current?.contentWindow?.postMessage({ type: 'ddc-overlay-scene', sources }, window.location.origin);
    outputIframeRef.current?.contentWindow?.postMessage({ type: 'ddc-overlay-scene', sources }, window.location.origin);
  }, [sources]);

  const storageKey = `ddc-overlay-layout-v1:${normalizedLogin}`;

  useEffect(() => {
    if (!normalizedLogin) return;
    try {
      const savedConfig = JSON.parse(window.localStorage.getItem(storageKey) || 'null') as SavedOverlayConfig | null;
      if (!savedConfig) return;
      if (savedConfig.editorMode === 'simple' || savedConfig.editorMode === 'advanced') setEditorMode(savedConfig.editorMode);
      const storedWidth = Number.isFinite(savedConfig.canvasWidth) ? Math.min(3840, Math.max(320, Number(savedConfig.canvasWidth))) : DEFAULT_CANVAS_WIDTH;
      const storedHeight = Number.isFinite(savedConfig.canvasHeight) ? Math.min(2160, Math.max(180, Number(savedConfig.canvasHeight))) : DEFAULT_CANVAS_HEIGHT;
      // Alte feste Layouts öffnen frei; bereits kopierte OBS-URLs bleiben unverändert.
      setLayout(savedConfig.editorVersion === 2 && (savedConfig.layout === 'box' || savedConfig.layout === 'bar') ? savedConfig.layout : 'canvas');
      if (savedConfig.theme === 'dark' || savedConfig.theme === 'light' || savedConfig.theme === 'accent') setTheme(savedConfig.theme);
      if (savedConfig.mode === 'all' || savedConfig.mode === 'standard' || savedConfig.mode === 'ranked' || savedConfig.mode === 'brawl') setMode(savedConfig.mode);
      if (Number.isFinite(savedConfig.opacity)) setOpacity(Math.min(100, Math.max(0, Math.round(Number(savedConfig.opacity)))));
      if (Number.isFinite(savedConfig.recentN)) setRecentN(Math.min(15, Math.max(1, Math.round(Number(savedConfig.recentN)))));
      if (Number.isFinite(savedConfig.radius)) setRadius(Math.min(32, Math.max(0, Math.round(Number(savedConfig.radius)))));
      const validHex = (value: unknown): value is string => typeof value === 'string' && /^#[0-9a-f]{6}$/i.test(value);
      if (validHex(savedConfig.accent)) setAccent(savedConfig.accent);
      if (validHex(savedConfig.background)) setBackground(savedConfig.background);
      if (validHex(savedConfig.text)) setText(savedConfig.text);
      if (savedConfig.modules && typeof savedConfig.modules === 'object') {
        setModules(current => {
          const next = { ...current };
          for (const key of Object.keys(DEFAULT_MODULES) as ModuleKey[]) {
            if (typeof savedConfig.modules?.[key] === 'boolean') next[key] = savedConfig.modules[key] as boolean;
          }
          return next;
        });
      }
      if (Number.isFinite(savedConfig.canvasWidth)) setCanvasWidth(storedWidth);
      if (Number.isFinite(savedConfig.canvasHeight)) setCanvasHeight(storedHeight);
      if (savedConfig.sources && typeof savedConfig.sources === 'object') {
        setSources(current => {
          const next = { ...current };
          for (const key of Object.keys(DEFAULT_CANVAS_SOURCES) as ModuleKey[]) {
            const source = savedConfig.sources?.[key];
            if (source && typeof source === 'object') next[key] = clampSource({ ...current[key], ...source }, storedWidth, storedHeight);
          }
          return next;
        });
      }
    } catch {
      // Beschädigte lokale Layouts bleiben folgenlos; die Standardansicht bleibt nutzbar.
    }
  }, [normalizedLogin, storageKey]);

  useEffect(() => {
    if (editingField?.key === selectedSource) return;
    const activeSource = sources[selectedSource] || DEFAULT_CANVAS_SOURCES[selectedSource];
    setSourceDrafts(current => ({ ...current, [selectedSource]: sourceDraft(activeSource) }));
  }, [editingField, selectedSource, sources]);

  useEffect(() => {
    if (editingCanvasDimension !== 'width') setCanvasWidthDraft(String(canvasWidth));
    if (editingCanvasDimension !== 'height') setCanvasHeightDraft(String(canvasHeight));
  }, [canvasHeight, canvasWidth, editingCanvasDimension]);

  useEffect(() => {
    type PointerPosition = Pick<PointerEvent, 'clientX' | 'clientY' | 'pointerId' | 'altKey'>;
    let lastPointer: PointerPosition | null = null;
    const handlePointerMove = (event: PointerPosition) => {
      const drag = dragRef.current;
      const box = marqueeRef.current;
      const preview = editorRef.current;
      if (!preview || (!drag && !box)) return;
      const bounds = preview.getBoundingClientRect();
      if (bounds.width <= 0 || bounds.height <= 0) return;
      if (box && event.pointerId === box.pointerId) {
        const x = Math.max(0, Math.min(canvasWidth, (event.clientX - bounds.left) / bounds.width * canvasWidth));
        const y = Math.max(0, Math.min(canvasHeight, (event.clientY - bounds.top) / bounds.height * canvasHeight));
        const rect = { x: Math.min(box.startX, x), y: Math.min(box.startY, y), width: Math.abs(x - box.startX), height: Math.abs(y - box.startY) };
        setMarquee(rect);
        const scene = sceneRef.current;
        const members = MODULES.filter(({ key }) => scene.modules[key] && selectionIntersects(rect, scene.sources[key])).map(({ key }) => key);
        const next = [...new Set([...box.initial, ...members])];
        setSelectedSources(next);
        if (next.length) setSelectedSource(next[next.length - 1]);
        return;
      }
      if (!drag || event.pointerId !== drag.pointerId) return;
      lastPointer = event;
      const dx = (event.clientX - drag.startX) / bounds.width * canvasWidth;
      const dy = (event.clientY - drag.startY) / bounds.height * canvasHeight;
      if (drag.mode === 'resize') {
        const next = resizeSource(drag.source, drag.source.width + dx, drag.source.height + dy, canvasWidth, canvasHeight);
        setSources(current => ({ ...current, [drag.key]: next }));
        return;
      }
      const snap = magnetRef.current && !event.altKey;
      const delta = moveSelection(drag.members.map(member => member.source), drag.targets, canvasWidth, canvasHeight, dx, dy,
        snap ? 6 * canvasWidth / bounds.width : 0, snap ? 6 * canvasHeight / bounds.height : 0);
      setSnapHits(delta.hits);
      setSources(current => {
        const next = { ...current };
        for (const { key, source } of drag.members) next[key] = { ...source, x: source.x + delta.dx, y: source.y + delta.dy };
        return next;
      });
    };
    const handlePointerUp = (event: PointerEvent) => {
      const drag = dragRef.current || marqueeRef.current;
      if (!drag || event.pointerId !== drag.pointerId) return;
      lastPointer = null;
      endGesture();
    };
    const handleAlt = (event: KeyboardEvent) => {
      if (event.key === 'Alt' && lastPointer && dragRef.current?.mode === 'move') {
        handlePointerMove({ clientX: lastPointer.clientX, clientY: lastPointer.clientY, pointerId: lastPointer.pointerId, altKey: event.type === 'keydown' });
      }
    };
    const handleBlur = () => {
      const active = dragRef.current || marqueeRef.current;
      if (active) handlePointerUp({ pointerId: active.pointerId } as PointerEvent);
    };
    window.addEventListener('pointermove', handlePointerMove);
    window.addEventListener('pointerup', handlePointerUp);
    window.addEventListener('pointercancel', handlePointerUp);
    window.addEventListener('lostpointercapture', handlePointerUp);
    window.addEventListener('keydown', handleAlt);
    window.addEventListener('keyup', handleAlt);
    window.addEventListener('blur', handleBlur);
    return () => {
      window.removeEventListener('pointermove', handlePointerMove);
      window.removeEventListener('pointerup', handlePointerUp);
      window.removeEventListener('pointercancel', handlePointerUp);
      window.removeEventListener('lostpointercapture', handlePointerUp);
      window.removeEventListener('keydown', handleAlt);
      window.removeEventListener('keyup', handleAlt);
      window.removeEventListener('blur', handleBlur);
      endGesture();
    };
  }, [canvasHeight, canvasWidth, endGesture]);

  const overlayUrl = useMemo(() => {
    const origin = typeof window === 'undefined' ? '' : window.location.origin;
    const params = new URLSearchParams();
    params.set('streamer', normalizedLogin);
    params.set('theme', theme);
    params.set('layout', layout);
    params.set('mode', mode);
    params.set('opacity', String(opacity));
    params.set('recent_n', String(recentN));
    params.set('accent', accent);
    params.set('background', background);
    params.set('text', text);
    params.set('radius', String(radius));
    if (layout === 'canvas') {
      params.set('scene_v', '2');
      params.set('canvas_w', String(canvasWidth));
      params.set('canvas_h', String(canvasHeight));
      params.set('scene', JSON.stringify(sources));
    }
    for (const { key } of MODULES) {
      params.set(key, modules[key] ? '1' : '0');
    }
    return `${origin}/twitch/overlay?${params.toString()}`;
  }, [normalizedLogin, theme, layout, mode, opacity, recentN, modules, accent, background, text, radius, canvasWidth, canvasHeight, sources]);
  // Geometrie geht über die Bridge: Drag/Resize lädt weder iframe noch Live-Werte neu.
  const previewUrl = new URL(overlayUrl, 'http://localhost');
  previewUrl.searchParams.delete('scene');
  previewUrl.searchParams.set('editor', '1');
  const debouncedUrl = useDebouncedValue(previewUrl.toString(), 300);
  const unsaved = retainedUrl !== null && retainedUrl !== overlayUrl;

  useEffect(() => {
    setCopied(false);
  }, [overlayUrl]);

  useLayoutEffect(() => {
    const host = previewHostRef.current;
    if (!host) return;
    const updateSize = () => {
      const bounds = host.getBoundingClientRect();
      setPreviewHostWidth(Math.max(1, bounds.width));
      // Beide Flächen passen in den Viewport, auch bei hohen festen Karten.
      setPreviewHostHeight(Math.max(1, Math.min(window.innerHeight - 150,
        expandedPreview ? (previewPanelRef.current?.clientHeight ?? 670) - 150 : 520)));
      setOutputHostWidth(Math.max(1, outputHostRef.current?.getBoundingClientRect().width ?? bounds.width));
    };
    updateSize();
    const observer = new ResizeObserver(updateSize);
    observer.observe(host);
    if (outputHostRef.current) observer.observe(outputHostRef.current);
    window.addEventListener('resize', updateSize);
    document.addEventListener('fullscreenchange', updateSize);
    return () => {
      observer.disconnect();
      window.removeEventListener('resize', updateSize);
      document.removeEventListener('fullscreenchange', updateSize);
    };
  }, [expandedPreview, normalizedLogin, editorMode, layout]);

  const toggleModule = (key: ModuleKey) => {
    endGesture();
    if (modules[key]) {
      const next = visibleSelection.filter(item => item !== key);
      setSelectedSources(next);
      if (next.length && selectedSource === key) setSelectedSource(next[next.length - 1]);
    }
    setModules((current) => ({ ...current, [key]: !current[key] }));
  };

  const resizeCanvas = (rawWidth: number, rawHeight: number) => {
    const nextWidth = Math.min(3840, Math.max(320, Math.round(Number.isFinite(rawWidth) ? rawWidth : canvasWidth)));
    const nextHeight = Math.min(2160, Math.max(180, Math.round(Number.isFinite(rawHeight) ? rawHeight : canvasHeight)));
    const widthRatio = nextWidth / canvasWidth;
    const heightRatio = nextHeight / canvasHeight;
    setSources(current => {
      const next = { ...current };
      for (const key of Object.keys(DEFAULT_CANVAS_SOURCES) as ModuleKey[]) {
        const source = current[key];
        next[key] = clampSource({
          x: source.x * widthRatio,
          y: source.y * heightRatio,
          width: source.width * widthRatio,
          height: source.height * heightRatio,
        }, nextWidth, nextHeight);
      }
      return next;
    });
    setCanvasWidth(nextWidth);
    setCanvasHeight(nextHeight);
  };

  const changeCanvasSize = (dimension: 'width' | 'height', rawValue: number) => {
    resizeCanvas(dimension === 'width' ? rawValue : canvasWidth, dimension === 'height' ? rawValue : canvasHeight);
  };

  const commitCanvasDimension = (dimension: 'width' | 'height') => {
    const rawValue = dimension === 'width' ? canvasWidthDraft : canvasHeightDraft;
    const parsedValue = Number(rawValue);
    if (Number.isFinite(parsedValue)) {
      changeCanvasSize(dimension, parsedValue);
    } else if (dimension === 'width') {
      setCanvasWidthDraft(String(canvasWidth));
    } else {
      setCanvasHeightDraft(String(canvasHeight));
    }
    setEditingCanvasDimension(null);
  };

  const beginSourceDrag = (event: React.PointerEvent<HTMLElement>, key: ModuleKey, mode: DragState['mode']) => {
    if (event.button !== 0 || dragRef.current || marqueeRef.current) return;
    event.preventDefault();
    event.stopPropagation();
    // Blur commits numeric drafts (including canvas dimensions). Finish that render
    // before capturing geometry; otherwise the first pointermove restores stale values.
    flushSync(() => previewPanelRef.current?.focus({ preventScroll: true }));
    const currentSources = sceneRef.current.sources;
    const source = currentSources[key] || DEFAULT_CANVAS_SOURCES[key];
    const bounds = editorRef.current?.getBoundingClientRect();
    if (!bounds) return;
    const additive = mode === 'move' && (event.ctrlKey || event.metaKey || event.shiftKey);
    const keys = additive ? toggleSelection(visibleSelection, key) : visibleSelection.includes(key) ? visibleSelection : [key];
    setSelectedSources(keys);
    setSelectedSource(keys.includes(key) ? key : keys[keys.length - 1] || key);
    // Removing a member is only a toggle, never a drag of the remaining group.
    if (!keys.includes(key)) return;
    setSnapHits([]);
    setActiveSource(key);
    setTouchSource(event.pointerType === 'touch' ? key : null);
    event.currentTarget.setPointerCapture(event.pointerId);
    dragRef.current = {
      pointerId: event.pointerId,
      target: event.currentTarget,
      key,
      mode,
      startX: event.clientX,
      startY: event.clientY,
      source,
      members: keys.map(member => ({ key: member, source: currentSources[member] })),
      targets: MODULES.filter(({ key: other }) => modules[other] && !keys.includes(other)).map(({ key: other }) => currentSources[other]),
    };
  };

  const beginMarquee = (event: React.PointerEvent<HTMLDivElement>) => {
    if (event.target !== event.currentTarget || event.button !== 0 || dragRef.current || marqueeRef.current) return;
    const bounds = editorRef.current?.getBoundingClientRect();
    if (!bounds || bounds.width <= 0 || bounds.height <= 0) return;
    event.preventDefault();
    previewPanelRef.current?.focus({ preventScroll: true });
    const initial = event.ctrlKey || event.metaKey || event.shiftKey ? visibleSelection : [];
    const startX = (event.clientX - bounds.left) / bounds.width * canvasWidth;
    const startY = (event.clientY - bounds.top) / bounds.height * canvasHeight;
    setSelectedSources(initial);
    setTouchSource(null);
    setSnapHits([]);
    setMarquee({ x: startX, y: startY, width: 0, height: 0 });
    event.currentTarget.setPointerCapture(event.pointerId);
    marqueeRef.current = { pointerId: event.pointerId, target: event.currentTarget, startX, startY, initial };
  };

  const centerSelection = (axis: 'x' | 'y') => {
    setSources(current => {
      const rects = visibleSelection.map(key => current[key]);
      const bounds = selectionBounds(rects);
      if (!bounds) return current;
      const delta = moveSelection(rects, [], canvasWidth, canvasHeight,
        axis === 'x' ? (canvasWidth - bounds.width) / 2 - bounds.x : 0,
        axis === 'y' ? (canvasHeight - bounds.height) / 2 - bounds.y : 0);
      const next = { ...current };
      for (const key of visibleSelection) next[key] = { ...current[key], x: current[key].x + delta.dx, y: current[key].y + delta.dy };
      return next;
    });
  };

  const updateSource = (key: ModuleKey, field: keyof OverlaySource, rawValue: number) => {
    setSources(current => ({
      ...current,
      [key]: field === 'width' || field === 'height'
        ? resizeSource(current[key], field === 'width' ? rawValue : current[key].width, field === 'height' ? rawValue : current[key].height, canvasWidth, canvasHeight)
        : clampSource({ ...current[key], [field]: rawValue }, canvasWidth, canvasHeight),
    }));
  };

  const commitSourceField = (key: ModuleKey, field: SourceField) => {
    const rawValue = sourceDrafts[key]?.[field] || "";
    const parsedValue = Number(rawValue);
    if (Number.isFinite(parsedValue)) {
      updateSource(key, field, parsedValue);
    } else {
      setSourceDrafts(current => ({ ...current, [key]: sourceDraft(sources[key]) }));
    }
    setEditingField(null);
  };

  const resetSource = (key: ModuleKey) => {
    setSources(current => ({ ...current, [key]: clampSource(DEFAULT_CANVAS_SOURCES[key], canvasWidth, canvasHeight) }));
  };

  const resetCanvas = () => {
    setCanvasWidth(DEFAULT_CANVAS_WIDTH);
    setCanvasHeight(DEFAULT_CANVAS_HEIGHT);
    setSources(DEFAULT_CANVAS_SOURCES);
    setSelectedSource('rank');
    setSelectedSources(['rank']);
  };

  const saveLayout = () => {
    try {
      window.localStorage.setItem(storageKey, JSON.stringify({
        editorVersion: 2,
        editorMode,
        layout,
        theme,
        mode,
        opacity,
        recentN,
        modules,
        accent,
        background,
        text,
        radius,
        canvasWidth,
        canvasHeight,
        sources,
      }));
      setRetainedUrl(overlayUrl);
      setSaved(true);
      setSaveFailed(false);
      window.setTimeout(() => setSaved(false), 1800);
    } catch {
      setSaved(false);
      setSaveFailed(true);
    }
  };

  const copyUrl = async () => {
    try {
      await navigator.clipboard.writeText(overlayUrl);
      setCopied(true);
      setRetainedUrl(overlayUrl);
      setCopyFailed(false);
      window.setTimeout(() => setCopied(false), 1800);
    } catch {
      setCopied(false);
      setCopyFailed(true);
      const input = document.getElementById('overlay-url') as HTMLInputElement | null;
      input?.focus(); input?.select();
    }
  };

  const renderWidth = layout === 'canvas' ? canvasWidth : layout === 'bar' ? 960 : 440;
  const renderHeight = layout === 'canvas' ? canvasHeight : layout === 'bar' ? 300 : 660;
  // Die unskalierten Hosts messen: eine skalierte Ausgabe darf ihre eigene
  // Breite nicht als Eingabe zurückführen. Editor und Ausgabe teilen den Fit.
  const availablePreviewWidth = editorMode === 'advanced' ? Math.min(previewHostWidth, outputHostWidth) : previewHostWidth;
  const canvasPreviewScale = Math.min(Math.max(1, availablePreviewWidth) / renderWidth, previewHostHeight / renderHeight);
  const canvasPreviewWidth = renderWidth * canvasPreviewScale;
  const canvasPreviewHeight = renderHeight * canvasPreviewScale;
  const recommendedSize = layout === 'bar' ? '960 × 300' : layout === 'canvas' ? `${canvasWidth} × ${canvasHeight}` : '440 × 660';
  const selected = sources[selectedSource] || DEFAULT_CANVAS_SOURCES[selectedSource];
  const selectedDraft = sourceDrafts[selectedSource] || sourceDraft(selected);
  const selectedLabel = MODULES.find(({ key }) => key === selectedSource)?.label || selectedSource;
  const groupBounds = selectionBounds(visibleSelection.map(key => sources[key]));

  if (!normalizedLogin) {
    return (
      <motion.section
        className="panel-card rounded-2xl p-5 md:p-6"
        initial={{ opacity: 0, y: 16 }}
        whileInView={{ opacity: 1, y: 0 }}
        viewport={{ once: true }}
        transition={{ duration: 0.32, delay: 0.14 }}
      >
        <div>
          <p className="mb-1 text-sm font-medium uppercase tracking-wider text-primary">
            Stream-Overlay
          </p>
          <h2 className="display-font mb-1 text-2xl font-bold text-white">
            Overlay noch nicht verfügbar
          </h2>
          <p className="text-sm text-text-secondary">
            Sobald dein Konto verbunden ist, kannst du dir hier dein Stream-Overlay zusammenstellen.
          </p>
        </div>
      </motion.section>
    );
  }

  return (
    <motion.section
      tabIndex={-1}
      onKeyDown={(event) => {
        const target = event.target;
        if (event.defaultPrevented || event.repeat || event.nativeEvent.isComposing ||
          !(target instanceof HTMLElement) ||
          event.ctrlKey || event.metaKey || event.shiftKey) return;
        if (event.key === 'Escape' && !event.altKey && expandedPreview) {
          // Native Auswahlmenüs verarbeiten Escape zuerst; geschlossener Feldfokus sperrt den Ausstieg nicht.
          if (target instanceof HTMLSelectElement && CSS.supports('selector(select:open)') && target.matches(':open')) return;
          endGesture();
          event.preventDefault();
          event.stopPropagation();
          if (fullscreen) void toggleFullscreen();
          else if (performance.now() - fullscreenExitRef.current > 250) {
            setTheaterMode(false);
            setFullscreenNotice('');
          }
          return;
        }
        if (target.isContentEditable || target.closest('input, select, textarea, [role="textbox"]')) return;
        if (event.key === 'Escape' && !event.altKey) {
          endGesture();
          setSelectedSources([]);
          setTouchSource(null);
          previewPanelRef.current?.focus({ preventScroll: true });
          return;
        }
        if (event.key.toLowerCase() === 't' && event.altKey && !fullscreen) {
          event.preventDefault();
          event.stopPropagation();
          endGesture();
          setFullscreenNotice('');
          setTheaterMode(current => !current);
          previewPanelRef.current?.focus({ preventScroll: true });
        } else if (event.key.toLowerCase() === 'f' && !event.altKey) {
          event.preventDefault();
          event.stopPropagation();
          void toggleFullscreen();
        }
      }}
      data-tour-id="onboarding-overlay"
      data-tour-ready="true"
      onPointerDownCapture={() => setTouchSource(null)}
      onPointerMoveCapture={(event) => { if (event.pointerType === 'mouse') setTouchSource(null); }}
      data-unsaved={unsaved}
      data-unsaved-hint="Dein Overlay ist angepasst. Kopiere zuerst die neue OBS-Adresse oder bestätige unten, dass du sie übernommen hast. Du kannst auch den Rundgang pausieren und hierbleiben."
      className="panel-card rounded-2xl p-5 md:p-6"
      initial={{ opacity: 0, y: 16 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true }}
      transition={{ duration: 0.32, delay: 0.14 }}
    >
      <div className="mb-5">
        <p className="mb-1 text-sm font-medium uppercase tracking-wider text-primary">
          Stream-Overlay
        </p>
        <h2 className="display-font mb-1 text-2xl font-bold text-white">
          Overlay für OBS zusammenstellen
        </h2>
        <p className="text-sm text-text-secondary">
          {editorMode === 'advanced' ? 'Zieh deine Module an die gewünschte Stelle. Strg oder ⌘ + Klick wählt mehrere aus; am Griff änderst du die Größe der aktiven Quelle.' : 'Wähle eine fertige Karte oder Leiste und passe Inhalte und Farbe an. Dein eigenes Layout bleibt beim Wechsel erhalten.'} Die Vorschau zeigt deine echten Spielwerte.
          Voraussetzung: ein über den Discord verknüpfter Steam-Account.
        </p>
      </div>

      <div className="mb-5 flex flex-wrap items-center gap-3">
        <div role="group" aria-label="Bedienmodus" className="inline-flex rounded-xl border border-border bg-background/60 p-1">
          {([{ value: 'simple', label: 'Einfach' }, { value: 'advanced', label: 'Erweitert' }] as const).map(item => <button key={item.value} type="button" aria-pressed={editorMode === item.value} onClick={() => { endGesture(); setEditorMode(item.value); }} className={`min-h-11 rounded-lg px-4 text-sm font-semibold focus-visible:outline-2 focus-visible:outline-primary ${editorMode === item.value ? 'bg-primary/15 text-primary' : 'text-text-secondary hover:text-white'}`}>{item.label}</button>)}
        </div>
        <p className="text-xs text-text-secondary">Der Modus ändert nur die Bedienung. Layout und OBS-Adresse bleiben gleich.</p>
      </div>

      {editorMode === 'simple' && <section className="mb-5 space-y-3 rounded-xl border border-border bg-background/40 p-4" aria-label="Fertige Vorlagen">
        <h3 className="text-sm font-semibold text-white">Vorlage auswählen</h3>
        {layout === 'canvas' && <p className="text-sm text-text-secondary">Aktuell: Eigenes Layout. Es bleibt erhalten, bis du ausdrücklich eine Vorlage auswählst. Unter „Erweitert“ kannst du es weiter bearbeiten.</p>}
        <div className="grid gap-3 sm:grid-cols-2">
          {([{ value: 'box', label: 'Feste Karte', detail: 'Kompakte Übersicht · 440 × 660' }, { value: 'bar', label: 'Feste Leiste', detail: 'Breites Overlay · 960 × 300' }] as const).map(preset => <button key={preset.value} type="button" aria-pressed={layout === preset.value} onClick={() => setLayout(preset.value)} className={`flex min-h-20 items-center gap-4 rounded-xl border p-4 text-left focus-visible:outline-2 focus-visible:outline-primary ${layout === preset.value ? 'border-primary bg-primary/10 text-white' : 'border-border bg-background text-text-secondary hover:border-primary/50'}`}><span aria-hidden="true" className={`shrink-0 rounded border-2 border-primary/60 bg-primary/10 ${preset.value === 'box' ? 'h-12 w-8' : 'h-6 w-16'}`} /><span><span className="block font-semibold">{preset.label}</span><span className="text-xs text-text-secondary">{preset.detail}</span></span></button>)}
        </div>
      </section>}

      <div className="grid min-w-0 items-start gap-4">
        <div
          ref={previewPanelRef}
          tabIndex={0}
          role="region"
          aria-label="Overlay-Baukasten und Live-Ausgabe"
          onPointerDown={(event) => {
            if (event.target instanceof HTMLElement && !event.target.closest('button, input, select, textarea, [contenteditable]')) {
              event.currentTarget.focus({ preventScroll: true });
            }
          }}
          className="min-w-0 flex flex-col gap-3 rounded-xl focus-visible:outline-2 focus-visible:outline-primary"
          style={fullscreen ? { width: '100%', height: '100dvh', padding: 16, background: '#090a0d', overflow: 'auto' } : theaterMode ? { height: 'max(240px, calc(100dvh - 12rem))' } : undefined}
        >
          <div className="flex shrink-0 flex-wrap items-center justify-between gap-2">
            <h3 className="text-sm font-semibold text-white">{editorMode === 'advanced' ? 'Baukasten & Live-Ausgabe' : 'Deine Vorschau'}</h3>
            <div className="flex flex-wrap items-center gap-2">
              {editorMode === 'advanced' && layout === 'canvas' && <>
                <span data-testid="overlay-selection-count" className="text-xs text-text-secondary">{visibleSelection.length} ausgewählt</span>
                <button type="button" aria-label="Magnet" aria-pressed={magnetEnabled} onClick={() => { magnetRef.current = !magnetEnabled; setMagnetEnabled(!magnetEnabled); setSnapHits([]); }} className="min-h-11 rounded-lg border border-border px-3 text-xs text-primary aria-pressed:border-primary aria-pressed:bg-primary/10">Magnet {magnetEnabled ? 'an' : 'aus'}</button>
              </>}
              <select aria-label="Vorschau-Hintergrund" value={previewBackdrop} onChange={event => setPreviewBackdrop(event.target.value as typeof previewBackdrop)} className="min-h-11 rounded-lg border border-border bg-background px-2 text-xs text-white"><option value="checker">Transparenz</option><option value="dark">Dunkle Szene</option><option value="light">Helle Szene</option></select>
              <button type="button" title="Kino-Modus (Alt+T)" aria-label="Kino-Modus (Alt+T)" aria-keyshortcuts="Alt+t" aria-pressed={theaterMode} disabled={fullscreen} onClick={() => { endGesture(); setTheaterMode(current => !current); setFullscreenNotice(''); }} className="inline-flex h-11 w-11 items-center justify-center rounded-lg border border-border text-white hover:border-primary hover:text-primary focus-visible:outline-2 focus-visible:outline-primary aria-pressed:border-primary aria-pressed:text-primary disabled:opacity-40">
                <RectangleHorizontal aria-hidden="true" className="h-5 w-5" />
              </button>
              <button type="button" title={fullscreen ? 'Vollbild beenden (F)' : 'Vollbild (F)'} aria-label={fullscreen ? 'Vollbild beenden (F)' : 'Vollbild (F)'} aria-keyshortcuts="f" aria-pressed={fullscreen} onClick={() => void toggleFullscreen()} className="inline-flex h-11 w-11 items-center justify-center rounded-lg border border-border text-white hover:border-primary hover:text-primary focus-visible:outline-2 focus-visible:outline-primary">
                {fullscreen ? <Minimize aria-hidden="true" className="h-5 w-5" /> : <Maximize aria-hidden="true" className="h-5 w-5" />}
              </button>
              {expandedPreview && <button type="button" title={fullscreen ? 'Vollbild schließen (Escape)' : 'Kino-Modus schließen (Escape)'} aria-label={fullscreen ? 'Vollbild schließen (Escape)' : 'Kino-Modus schließen (Escape)'} onClick={() => { endGesture(); if (fullscreen) void toggleFullscreen(); else { setTheaterMode(false); setFullscreenNotice(''); previewPanelRef.current?.focus({ preventScroll: true }); } }} className="inline-flex h-11 w-11 items-center justify-center rounded-lg border border-border text-white hover:border-primary hover:text-primary focus-visible:outline-2 focus-visible:outline-primary"><X aria-hidden="true" className="h-5 w-5" /></button>}
            </div>
          </div>
          {fullscreenNotice && <p role="status" className="shrink-0 text-xs text-text-secondary">{fullscreenNotice}</p>}
          <div className={`grid min-w-0 items-start content-start gap-4 ${editorMode === 'advanced' ? 'lg:grid-cols-2' : ''} ${expandedPreview ? 'min-h-0 flex-1 overflow-auto' : ''}`}>
          <div className="flex min-w-0 flex-col gap-2">
          <h4 className="text-sm font-semibold text-primary">{editorMode === 'advanced' ? 'Baukasten · Editor' : 'So erscheint dein Overlay in OBS'}</h4>
          <div ref={previewHostRef} className="min-w-0 shrink-0">
          <div
            ref={editorRef}
            className={`relative overflow-hidden rounded-xl ring-1 ring-border bg-background/60 ${layout === 'canvas' ? 'touch-none select-none' : ''}`}
            style={{
              ...CHECKER_STYLE,
              width: canvasPreviewWidth, height: canvasPreviewHeight, marginInline: 'auto', flexShrink: 0,
            }}
          >
            <iframe
              ref={iframeRef}
              data-testid="overlay-editor-frame"
              onLoad={syncPreview}
              src={debouncedUrl}
              title="Overlay-Editor mit echten Spielwerten"
              style={{ width: renderWidth, height: renderHeight, transform: `scale(${canvasPreviewScale})`, transformOrigin: 'top left' }}
              className="pointer-events-none absolute left-0 top-0 block max-w-none border-0 bg-transparent"
            />
            {editorMode === 'advanced' && layout === 'canvas' && (
              <div className="absolute inset-0 z-10" data-testid="overlay-editor-controls" onPointerDown={beginMarquee}>
                {MODULES.map(({ key, label }) => {
                  if (!modules[key]) return null;
                  const source = sources[key];
                  return (
                    <div
                      key={key}
                      className="group/source absolute"
                      data-chrome={visibleSelection.includes(key) || activeSource === key || touchSource === key ? 'visible' : undefined}
                      style={{ left: `${source.x / canvasWidth * 100}%`, top: `${source.y / canvasHeight * 100}%`, width: `${source.width / canvasWidth * 100}%`, height: `${source.height / canvasHeight * 100}%`, zIndex: visibleSelection.includes(key) ? 2 : 1 }}
                    >
                      <button
                        type="button"
                        aria-label={`${label} verschieben und auswählen`}
                        aria-pressed={visibleSelection.includes(key)}
                        title={`${label}: ziehen zum Verschieben`}
                        onClick={(event) => { if (event.detail === 0) selectSource(key, event.ctrlKey || event.metaKey || event.shiftKey); }}
                        onPointerDown={(event) => beginSourceDrag(event, key, 'move')}
                        className="absolute inset-0 touch-none cursor-move rounded border-2 border-transparent text-left group-hover/source:border-primary group-has-[:focus-visible]/source:border-primary group-data-[chrome=visible]/source:border-primary"
                      >
                        <span className="pointer-events-none absolute left-0 bottom-full mb-1 whitespace-nowrap rounded bg-background/95 px-2 py-1 text-xs font-semibold text-primary opacity-0 group-hover/source:opacity-100 group-has-[:focus-visible]/source:opacity-100 group-data-[chrome=visible]/source:opacity-100">{label}</span>
                      </button>
                      <button
                        type="button"
                        aria-label={`${label}: Größe ändern`}
                        onPointerDown={(event) => beginSourceDrag(event, key, 'resize')}
                        onClick={(event) => { if (event.detail === 0) { if (!visibleSelection.includes(key)) selectSource(key); else setSelectedSource(key); } }}
                        className="pointer-events-none absolute bottom-0 right-0 flex h-6 w-6 touch-none cursor-se-resize items-center justify-center rounded-tl border border-primary bg-primary text-black opacity-0 group-hover/source:pointer-events-auto group-hover/source:opacity-100 group-has-[:focus-visible]/source:pointer-events-auto group-has-[:focus-visible]/source:opacity-100 group-data-[chrome=visible]/source:pointer-events-auto group-data-[chrome=visible]/source:opacity-100"
                        title={`${label}: ziehen zum Vergrößern oder Verkleinern`}
                      ><span aria-hidden="true">↘</span></button>
                    </div>
                  );
                })}
              </div>
            )}
            {editorMode === 'advanced' && layout === 'canvas' && groupBounds && <OverlayCanvasGuides source={groupBounds} canvasWidth={canvasWidth} canvasHeight={canvasHeight} scale={canvasPreviewScale} />}
            {editorMode === 'advanced' && layout === 'canvas' && visibleSelection.length > 1 && groupBounds && <div data-testid="overlay-group-bounds" aria-hidden="true" className="pointer-events-none absolute z-20 border border-dashed border-primary" style={{ left: `${groupBounds.x / canvasWidth * 100}%`, top: `${groupBounds.y / canvasHeight * 100}%`, width: `${groupBounds.width / canvasWidth * 100}%`, height: `${groupBounds.height / canvasHeight * 100}%` }} />}
            {editorMode === 'advanced' && layout === 'canvas' && marquee && <div data-testid="overlay-selection-box" aria-hidden="true" className="pointer-events-none absolute z-30 border border-primary bg-primary/15" style={{ left: `${marquee.x / canvasWidth * 100}%`, top: `${marquee.y / canvasHeight * 100}%`, width: `${marquee.width / canvasWidth * 100}%`, height: `${marquee.height / canvasHeight * 100}%` }} />}
            {editorMode === 'advanced' && layout === 'canvas' && snapHits.length > 0 && <svg data-testid="overlay-snap-guides" aria-hidden="true" className="pointer-events-none absolute inset-0 z-30" width="100%" height="100%">
              {snapHits.map(hit => <line key={hit.axis} data-axis={hit.axis} data-position={hit.position} x1={hit.axis === 'x' ? `${hit.position / canvasWidth * 100}%` : 0} x2={hit.axis === 'x' ? `${hit.position / canvasWidth * 100}%` : '100%'} y1={hit.axis === 'y' ? `${hit.position / canvasHeight * 100}%` : 0} y2={hit.axis === 'y' ? `${hit.position / canvasHeight * 100}%` : '100%'} stroke="#4ade80" strokeWidth={1.5} />)}
            </svg>}
          </div>
          </div>
          {editorMode === 'advanced' && layout === 'canvas' && groupBounds && <div className="flex flex-wrap items-center gap-2 text-xs text-text-secondary">
            <span>{visibleSelection.length > 1 ? `${visibleSelection.length} Module` : MODULES.find(({ key }) => key === visibleSelection[0])?.label} · {groupBounds.width} × {groupBounds.height} px</span>
            <button type="button" onClick={() => centerSelection('x')} className="min-h-9 rounded border border-border px-2 text-primary">Horizontal zentrieren</button>
            <button type="button" onClick={() => centerSelection('y')} className="min-h-9 rounded border border-border px-2 text-primary">Vertikal zentrieren</button>
            <span data-testid="overlay-center-status">Horizontal {Math.abs(groupBounds.x + groupBounds.width / 2 - canvasWidth / 2) <= 0.5 ? 'mittig' : 'nicht mittig'} · Vertikal {Math.abs(groupBounds.y + groupBounds.height / 2 - canvasHeight / 2) <= 0.5 ? 'mittig' : 'nicht mittig'}</span>
          </div>}
          <p hidden={editorMode === 'simple'} className="text-xs text-text-secondary">Strg/⌘/Shift + Klick: Auswahl ändern. Rahmen auf freier Fläche ziehen, mit Strg/⌘/Shift ergänzen. Alt: ohne Magnet. Größengriff: nur ein Modul. Freier Klick oder Escape: Auswahl aufheben.</p>
          </div>
          <div ref={outputHostRef} hidden={editorMode === 'simple'} className="flex min-w-0 flex-col gap-2">
            <h4 className="text-sm font-semibold text-white">Vorschau · So sieht es im Stream aus</h4>
            <div data-testid="overlay-clean-output" className="relative min-w-0 shrink-0 overflow-hidden rounded-xl ring-1 ring-border" style={{ ...(previewBackdrop === 'checker' ? CHECKER_STYLE : { background: previewBackdrop === 'dark' ? '#090a0d' : '#d8dce1' }), width: canvasPreviewWidth, height: canvasPreviewHeight, marginInline: 'auto' }}>
              <iframe ref={outputIframeRef} data-testid="overlay-output-frame" onLoad={syncPreview} src={debouncedUrl} title="Overlay-Live-Ausgabe ohne Bearbeitungshilfen" className="pointer-events-none absolute left-0 top-0 block max-w-none border-0 bg-transparent" style={{ width: renderWidth, height: renderHeight, transform: `scale(${canvasPreviewScale})`, transformOrigin: 'top left' }} />
            </div>
            <p className="text-xs text-text-secondary">Echte Spielwerte, laufend aktualisiert. Ohne verknüpftes Steam-Konto bleiben die Werte leer.</p>
          </div>
          </div>
        </div>

        <details hidden={expandedPreview} open={editorMode === 'simple' || undefined} className={`min-w-0 rounded-xl border border-border p-3 ${editorMode === 'advanced' ? 'lg:w-[calc(50%-0.5rem)]' : ''}`}>
        <summary className="cursor-pointer text-sm font-semibold text-primary">Optionen · Module, Farben und OBS-Adresse</summary>
        <div className="mt-3 min-w-0 space-y-4">
          <button hidden={editorMode === 'simple'} type="button" aria-pressed={layout === 'canvas'} onClick={() => setLayout('canvas')} className="min-h-11 w-full rounded-lg border border-primary bg-primary/15 px-4 py-2 text-sm font-semibold text-primary hover:bg-primary/25">
            Module frei bearbeiten
          </button>
          {editorMode === 'advanced' && layout === 'canvas' && hasSelection && (
              <div className="rounded-lg border border-border bg-background/60 p-3">
                <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
                  <div>
                    <h3 className="text-sm font-semibold text-white">{visibleSelection.length > 1 ? 'Aktive Quelle der Gruppe' : 'Ausgewählte Quelle'}: {selectedLabel}</h3>
                    <p className="text-xs text-text-secondary">X/Y = Position · Breite/Höhe = Größe{visibleSelection.length > 1 ? ' · Gilt nur für die aktive Quelle, die Gruppe bleibt ausgewählt.' : ''}</p>
                  </div>
                  <button type="button" onClick={() => resetSource(selectedSource)} className="min-h-10 rounded-lg border border-border px-3 text-xs font-semibold text-text-secondary hover:border-primary hover:text-primary">Quelle zurücksetzen</button>
                </div>
                <div className="grid grid-cols-2 gap-3">
                  {(['x', 'y', 'width', 'height'] as const).map((field) => (
                    <label key={field} className="text-xs font-semibold uppercase tracking-wider text-text-secondary">
                      {field === 'width' ? 'Breite' : field === 'height' ? 'Höhe' : field.toUpperCase()}
                      <input type="number" min={field === 'width' ? 120 : field === 'height' ? 48 : 0} max={field === 'width' ? canvasWidth - selected.x : field === 'height' ? canvasHeight - selected.y : field === 'x' ? canvasWidth - selected.width : canvasHeight - selected.height} step={1} value={selectedDraft[field]} onFocus={() => setEditingField({ key: selectedSource, field })} onChange={(event) => setSourceDrafts(current => ({ ...current, [selectedSource]: { ...current[selectedSource], [field]: event.target.value } }))} onBlur={() => commitSourceField(selectedSource, field)} className="mt-1 block w-full rounded-lg border border-border bg-background px-2 py-2 font-mono text-sm font-normal normal-case tracking-normal text-white" />
                    </label>
                  ))}
                </div>
              </div>
          )}
          {/* Stil, Layout & Spielmodus */}
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-2">
              <label htmlFor="overlay-theme" className="block text-sm font-semibold text-white">
                Stil
              </label>
              <select
                id="overlay-theme"
                value={theme}
                onChange={(event) => { const next = event.target.value as OverlayTheme; setTheme(next); setAccent(next === 'light' ? '#765321' : next === 'accent' ? '#a78bfa' : '#d6b56c'); setBackground(next === 'light' ? '#f5f3ef' : next === 'accent' ? '#161020' : '#0d0f14'); setText(next === 'light' ? '#17191f' : '#f4f7fb'); }}
                className="w-full rounded-lg border border-border bg-background/70 px-3 py-2 text-sm font-medium text-white outline-none transition-colors focus:border-border-hover"
              >
                {THEMES.map(({ value, label }) => (
                  <option key={value} value={value}>
                    {label}
                  </option>
                ))}
              </select>
            </div>

            <div hidden={editorMode === 'simple'} className="space-y-2">
              <label htmlFor="overlay-layout" className="block text-sm font-semibold text-white">
                Layout
              </label>
              <select
                id="overlay-layout"
                value={layout}
                onChange={(event) => setLayout(event.target.value as OverlayLayout)}
                className="w-full rounded-lg border border-border bg-background/70 px-3 py-2 text-sm font-medium text-white outline-none transition-colors focus:border-border-hover"
              >
                {LAYOUTS.map(({ value, label }) => (
                  <option key={value} value={value}>
                    {label}
                  </option>
                ))}
              </select>
            </div>

            <div className="space-y-2">
              <label htmlFor="overlay-mode" className="block text-sm font-semibold text-white">
                Spielmodus
              </label>
              <select
                id="overlay-mode"
                value={mode}
                onChange={(event) => setMode(event.target.value as OverlayMode)}
                className="w-full rounded-lg border border-border bg-background/70 px-3 py-2 text-sm font-medium text-white outline-none transition-colors focus:border-border-hover"
              >
                {MODES.map(({ value, label }) => (
                  <option key={value} value={value}>
                    {label}
                  </option>
                ))}
              </select>
            </div>
          </div>

          <fieldset hidden={editorMode === 'simple'} className="space-y-3">
            <legend className="text-sm font-semibold text-white">Deine Farben</legend>
            <div className="flex flex-wrap gap-2">
              {[
                { name: 'Nachtgold', accent: '#d6b56c', background: '#0d0f14', text: '#f4f7fb' },
                { name: 'Eisblau', accent: '#67d8f3', background: '#101922', text: '#f3faff' },
                { name: 'Flieder', accent: '#bca1ff', background: '#181322', text: '#f8f3ff' },
                { name: 'Papier', accent: '#765321', background: '#f5f3ef', text: '#17191f' },
              ].map(preset => <button key={preset.name} type="button" onClick={() => { setAccent(preset.accent); setBackground(preset.background); setText(preset.text); setTheme(preset.name === 'Papier' ? 'light' : 'dark'); }} className="inline-flex min-h-11 items-center gap-2 rounded-lg border border-border bg-background/60 px-3 text-sm text-white hover:border-primary focus-visible:outline-2 focus-visible:outline-primary"><span className="h-3 w-3 rounded-full" style={{ background: preset.accent }} />{preset.name}</button>)}
            </div>
            <div className="grid gap-2">
              {[{ key: 'accent', label: 'Akzent', value: accent, change: setAccent }, { key: 'background', label: 'Hintergrund', value: background, change: setBackground }, { key: 'text', label: 'Schrift', value: text, change: setText }].map(color => <label key={color.key} className="flex min-h-16 items-center gap-3 rounded-xl border border-border bg-background/60 p-3"><input aria-label={color.label} type="color" value={color.value} onChange={event => color.change(event.target.value)} className="h-9 w-10 cursor-pointer border-0 bg-transparent" /><span className="text-sm text-white">{color.label}<span className="block font-mono text-xs text-text-secondary">{color.value.toUpperCase()}</span></span></label>)}
            </div>
            <label className="block text-sm text-white" htmlFor="overlay-radius">Rundung <span className="text-text-secondary">{radius} px</span><input id="overlay-radius" type="range" min={0} max={32} value={radius} onChange={event => setRadius(Number(event.target.value))} className="mt-2 block w-full accent-primary" /></label>
          </fieldset>

          {/* Module */}
          <fieldset className="space-y-3">
            <legend className="text-sm font-semibold text-white">Inhalte</legend>
            <div className="grid gap-2">
              {MODULES.map(({ key, label }) => {
                const enabled = modules[key];
                return (
                  <div
                    key={key}
                    className="soft-elevate flex items-center justify-between gap-3 rounded-xl border border-border bg-background/60 p-3"
                  >
                    <span className="text-sm font-medium text-white">{label}</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={enabled}
                      aria-label={label}
                      onClick={() => toggleModule(key)}
                      className={`relative inline-flex h-6 w-11 shrink-0 items-center rounded-full transition-colors ${
                        enabled ? 'bg-primary' : 'bg-border'
                      }`}
                    >
                      <span
                        className={`inline-block h-4 w-4 transform rounded-full bg-white transition-[transform,translate,scale] ${
                          enabled ? 'translate-x-6' : 'translate-x-1'
                        }`}
                      />
                    </button>
                  </div>
                );
              })}
            </div>
          </fieldset>

          {/* Slider */}
          <div hidden={editorMode === 'simple'} className="grid gap-4">
            <div className="space-y-2">
              <label
                htmlFor="overlay-recent-n"
                className="flex items-center justify-between text-sm font-semibold text-white"
              >
                <span>Anzahl im Verlauf</span>
                <span className="text-text-secondary">{recentN}</span>
              </label>
              <input
                id="overlay-recent-n"
                type="range"
                min={1}
                max={15}
                step={1}
                value={recentN}
                onChange={(event) => setRecentN(Number(event.target.value))}
                disabled={!modules.recent}
                className="w-full accent-primary disabled:cursor-not-allowed disabled:opacity-50"
              />
            </div>

            <div className="space-y-2">
              <label
                htmlFor="overlay-opacity"
                className="flex items-center justify-between text-sm font-semibold text-white"
              >
                <span>Hintergrund-Deckkraft</span>
                <span className="text-text-secondary">{opacity}%</span>
              </label>
              <input
                id="overlay-opacity"
                type="range"
                min={0}
                max={100}
                step={1}
                value={opacity}
                onChange={(event) => setOpacity(Number(event.target.value))}
                className="w-full accent-primary"
              />
            </div>
          </div>

          {editorMode === 'advanced' && layout === 'canvas' && (
            <fieldset className="space-y-4 rounded-xl border border-primary/30 bg-background/50 p-4">
              <legend className="px-1 text-sm font-semibold text-white">OBS-Leinwand und Quellen</legend>
              <p className="text-sm leading-relaxed text-text-secondary">
                Stell hier deine OBS-Leinwand ein. Zieh die Quellen in der Vorschau an die richtige Stelle oder ändere ihre Breite und Höhe ganz genau.
              </p>

              <div className="flex flex-wrap gap-2">
                {CANVAS_PRESETS.map((preset) => (
                  <button
                    key={preset.label}
                    type="button"
                    onClick={() => resizeCanvas(preset.width, preset.height)}
                    className="min-h-10 rounded-lg border border-border bg-background/70 px-3 text-sm font-semibold text-white transition-colors hover:border-primary hover:text-primary"
                  >
                    {preset.label} · {preset.width} × {preset.height}
                  </button>
                ))}
              </div>

              <div className="grid grid-cols-2 gap-3">
                <label className="text-sm font-medium text-white">
                  Leinwand-Breite
                  <input type="number" min={320} max={3840} step={1} value={canvasWidthDraft} onFocus={() => setEditingCanvasDimension('width')} onChange={(event) => setCanvasWidthDraft(event.target.value)} onBlur={() => commitCanvasDimension('width')} className="mt-1 block w-full rounded-lg border border-border bg-background/70 px-3 py-2 font-mono text-sm text-white" />
                </label>
                <label className="text-sm font-medium text-white">
                  Leinwand-Höhe
                  <input type="number" min={180} max={2160} step={1} value={canvasHeightDraft} onFocus={() => setEditingCanvasDimension('height')} onChange={(event) => setCanvasHeightDraft(event.target.value)} onBlur={() => commitCanvasDimension('height')} className="mt-1 block w-full rounded-lg border border-border bg-background/70 px-3 py-2 font-mono text-sm text-white" />
                </label>
              </div>



              <div>
                <div className="mb-2 flex items-center justify-between gap-2">
                  <h3 className="text-sm font-semibold text-white">Quellen</h3>
                  <button type="button" onClick={resetCanvas} className="text-xs font-semibold text-text-secondary underline underline-offset-4 hover:text-primary">Leinwand zurücksetzen</button>
                </div>
                <div className="grid gap-2">
                  {MODULES.map(({ key, label }) => {
                    const source = sources[key];
                    return (
                      <button
                        key={key}
                        type="button"
                        disabled={!modules[key]}
                        title={!modules[key] ? 'Diese Quelle zuerst unter Inhalte einschalten.' : undefined}
                        aria-pressed={visibleSelection.includes(key)}
                        onClick={(event) => selectSource(key, event.ctrlKey || event.metaKey || event.shiftKey)}
                        className={`flex min-h-11 items-center justify-between gap-3 rounded-lg border px-3 text-left text-sm transition-colors disabled:cursor-not-allowed disabled:opacity-40 ${visibleSelection.includes(key) ? 'border-primary bg-primary/10 text-white' : 'border-border bg-background/50 text-text-secondary hover:border-primary/60'}`}
                      >
                        <span className="min-w-0 truncate">{label}</span>
                        <span className="shrink-0 font-mono text-xs">{Math.round(source.width)} × {Math.round(source.height)}</span>
                      </button>
                    );
                  })}
                </div>
              </div>
            </fieldset>
          )}

          {/* URL */}
          <div className="space-y-2">
            <label htmlFor="overlay-url" className="block text-sm font-semibold text-white">
              Deine Overlay-URL
            </label>
            <div className="flex flex-col gap-2">
              <input
                id="overlay-url"
                readOnly
                value={overlayUrl}
                className="min-w-0 flex-1 rounded-lg border border-border bg-background/70 px-3 py-2 font-mono text-xs text-text-secondary outline-none"
              />
              <button
                type="button"
                onClick={() => void copyUrl()}
                className="inline-flex items-center justify-center gap-2 rounded-lg border border-primary/40 bg-primary/10 px-4 py-2 text-sm font-semibold text-primary transition-colors hover:border-primary/60 hover:bg-primary/20"
              >
                {copied ? <Check className="h-4 w-4" /> : <Copy className="h-4 w-4" />}
                {copied ? 'Kopiert!' : 'URL kopieren'}
              </button>
            </div>
          </div>

          {copyFailed && <p role="alert" className="text-sm text-warning">Die Adresse konnte nicht kopiert werden. Kopiere sie aus dem markierten Feld und übernimm sie in OBS.</p>}
            <div className="flex flex-wrap items-center gap-3 rounded-lg border border-primary/25 bg-primary/5 px-3 py-2">
              <button type="button" onClick={saveLayout} className="min-h-11 rounded-lg border border-primary/50 bg-primary/15 px-4 text-sm font-semibold text-primary transition-colors hover:bg-primary/25">
                {saved ? 'Layout gespeichert' : 'Layout speichern'}
              </button>
              <span className="text-xs text-text-secondary">Dein Layout wird in diesem Browser gemerkt. Die fertige URL kannst du danach in OBS einfügen.</span>
            </div>
          {saveFailed && <p role="alert" className="text-sm text-warning">Das Layout konnte in diesem Browser nicht gespeichert werden.</p>}
          {unsaved && <button type="button" onClick={() => setRetainedUrl(overlayUrl)} className="min-h-11 text-sm text-primary underline underline-offset-4">Adresse in OBS übernommen</button>}

          {/* OBS */}
          <div className="space-y-2">
            <h3 className="text-sm font-semibold text-white">So fügst du es in OBS ein</h3>
            <ol className="list-decimal space-y-1.5 pl-5 text-sm text-text-secondary">
              <li>Klick in OBS unten bei „Quellen" auf das Plus und wähle „Browser".</li>
              <li>Vergib einen Namen (z. B. „Deadlock-Stats") und bestätige mit OK.</li>
              <li>Füge die obige Overlay-URL in das Feld „URL" ein.</li>
              <li>{layout === 'canvas' ? `Stell die Browser-Quelle auf ${canvasWidth} × ${canvasHeight}, passend zur Leinwand oben.` : 'Stell die Größe der Browser-Quelle passend zum Layout ein (siehe Empfehlung unten).'}</li>
              <li>Zieh die Quelle an die gewünschte Stelle — sie aktualisiert sich automatisch.</li>
            </ol>
            <div className="flex items-center justify-between gap-3 rounded-lg border border-border bg-background/60 px-3 py-2 text-sm">
              <span className="font-medium text-text-secondary">Empfohlene OBS-Größe</span>
              <span className="font-mono font-semibold text-white">{recommendedSize}</span>
            </div>
          </div>
        </div>
        </details>
      </div>
    </motion.section>
  );
}
