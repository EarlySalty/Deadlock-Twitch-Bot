import { useEffect, useMemo, useRef, useState, type CSSProperties } from 'react';
import { motion } from 'framer-motion';
import { Check, Copy } from 'lucide-react';

type OverlayTheme = 'dark' | 'light' | 'accent';
type OverlayLayout = 'box' | 'bar' | 'canvas';
type OverlayMode = 'all' | 'standard' | 'brawl';
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
  { value: 'box', label: 'Karte' },
  { value: 'bar', label: 'Leiste' },
  { value: 'canvas', label: 'Freie OBS-Leinwand' },
];

const MODES: Array<{ value: OverlayMode; label: string }> = [
  { value: 'all', label: 'Alle Modi' },
  { value: 'standard', label: 'Standard' },
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
  key: ModuleKey;
  mode: 'move' | 'resize';
  startX: number;
  startY: number;
  source: OverlaySource;
};

function clampSource(source: OverlaySource, canvasWidth: number, canvasHeight: number): OverlaySource {
  const width = Math.min(canvasWidth, Math.max(120, Math.round(source.width)));
  const height = Math.min(canvasHeight, Math.max(48, Math.round(source.height)));
  return {
    x: Math.min(Math.max(0, Math.round(source.x)), Math.max(0, canvasWidth - width)),
    y: Math.min(Math.max(0, Math.round(source.y)), Math.max(0, canvasHeight - height)),
    width,
    height,
  };
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
  const [layout, setLayout] = useState<OverlayLayout>('box');
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
  const [previewHostWidth, setPreviewHostWidth] = useState(560);
  const [previewBackdrop, setPreviewBackdrop] = useState<'checker' | 'dark' | 'light'>('checker');
  const [copied, setCopied] = useState(false);
  const [saved, setSaved] = useState(false);
  const [saveFailed, setSaveFailed] = useState(false);
  const [retainedUrl, setRetainedUrl] = useState<string | null>(null);
  const [copyFailed, setCopyFailed] = useState(false);
  const editorRef = useRef<HTMLDivElement>(null);
  const previewHostRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<DragState | null>(null);

  const storageKey = `ddc-overlay-layout-v1:${normalizedLogin}`;

  useEffect(() => {
    if (!normalizedLogin) return;
    try {
      const savedConfig = JSON.parse(window.localStorage.getItem(storageKey) || 'null') as SavedOverlayConfig | null;
      if (!savedConfig) return;
      const storedWidth = Number.isFinite(savedConfig.canvasWidth) ? Math.min(3840, Math.max(320, Number(savedConfig.canvasWidth))) : DEFAULT_CANVAS_WIDTH;
      const storedHeight = Number.isFinite(savedConfig.canvasHeight) ? Math.min(2160, Math.max(180, Number(savedConfig.canvasHeight))) : DEFAULT_CANVAS_HEIGHT;
      if (savedConfig.layout === 'box' || savedConfig.layout === 'bar' || savedConfig.layout === 'canvas') setLayout(savedConfig.layout);
      if (savedConfig.theme === 'dark' || savedConfig.theme === 'light' || savedConfig.theme === 'accent') setTheme(savedConfig.theme);
      if (savedConfig.mode === 'all' || savedConfig.mode === 'standard' || savedConfig.mode === 'brawl') setMode(savedConfig.mode);
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
    const handlePointerMove = (event: PointerEvent) => {
      const drag = dragRef.current;
      const preview = editorRef.current;
      if (!drag || !preview) return;
      const bounds = preview.getBoundingClientRect();
      if (bounds.width <= 0 || bounds.height <= 0) return;
      const dx = (event.clientX - drag.startX) / bounds.width * canvasWidth;
      const dy = (event.clientY - drag.startY) / bounds.height * canvasHeight;
      const next = drag.mode === 'move'
        ? { ...drag.source, x: drag.source.x + dx, y: drag.source.y + dy }
        : { ...drag.source, width: drag.source.width + dx, height: drag.source.height + dy };
      setSources(current => ({ ...current, [drag.key]: clampSource(next, canvasWidth, canvasHeight) }));
    };
    const handlePointerUp = () => {
      dragRef.current = null;
    };
    window.addEventListener('pointermove', handlePointerMove);
    window.addEventListener('pointerup', handlePointerUp);
    return () => {
      window.removeEventListener('pointermove', handlePointerMove);
      window.removeEventListener('pointerup', handlePointerUp);
    };
  }, [canvasHeight, canvasWidth]);

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
      params.set('canvas_w', String(canvasWidth));
      params.set('canvas_h', String(canvasHeight));
      params.set('scene', JSON.stringify(sources));
    }
    for (const { key } of MODULES) {
      params.set(key, modules[key] ? '1' : '0');
    }
    return `${origin}/twitch/overlay?${params.toString()}`;
  }, [normalizedLogin, theme, layout, mode, opacity, recentN, modules, accent, background, text, radius, canvasWidth, canvasHeight, sources]);
  const debouncedUrl = useDebouncedValue(overlayUrl, 300);
  const customized = accent !== '#d6b56c' || background !== '#0d0f14' || text !== '#f4f7fb' || radius !== 18 || theme !== 'dark' || layout !== 'box' || mode !== 'all' || opacity !== 85 || recentN !== 10 || canvasWidth !== DEFAULT_CANVAS_WIDTH || canvasHeight !== DEFAULT_CANVAS_HEIGHT || MODULES.some(({key}) => modules[key] !== DEFAULT_MODULES[key]);
  const unsaved = customized && retainedUrl !== null && retainedUrl !== overlayUrl;

  useEffect(() => {
    setCopied(false);
  }, [overlayUrl]);

  useEffect(() => {
    const host = previewHostRef.current;
    if (!host) return;
    const updateWidth = () => setPreviewHostWidth(Math.max(1, host.getBoundingClientRect().width));
    updateWidth();
    const observer = new ResizeObserver(updateWidth);
    observer.observe(host);
    return () => observer.disconnect();
  }, []);

  const toggleModule = (key: ModuleKey) => {
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
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    const source = sources[key] || DEFAULT_CANVAS_SOURCES[key];
    const bounds = editorRef.current?.getBoundingClientRect();
    if (!bounds) return;
    setSelectedSource(key);
    dragRef.current = {
      key,
      mode,
      startX: event.clientX,
      startY: event.clientY,
      source,
    };
  };

  const updateSource = (key: ModuleKey, field: keyof OverlaySource, rawValue: number) => {
    setSources(current => ({
      ...current,
      [key]: clampSource({ ...current[key], [field]: Number.isFinite(rawValue) ? rawValue : current[key][field] }, canvasWidth, canvasHeight),
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
  };

  const saveLayout = () => {
    try {
      window.localStorage.setItem(storageKey, JSON.stringify({
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

  const maxCanvasPreviewWidth = Math.max(1, Math.min(previewHostWidth, 560));
  const canvasPreviewScale = Math.min(maxCanvasPreviewWidth / canvasWidth, 560 / canvasHeight);
  const canvasPreviewWidth = Math.max(1, Math.round(canvasWidth * canvasPreviewScale));
  const canvasPreviewHeight = Math.max(1, Math.round(canvasHeight * canvasPreviewScale));
  const previewHeight = layout === 'bar' ? 300 : layout === 'canvas' ? canvasPreviewHeight : 660;
  const recommendedSize = layout === 'bar' ? '960 × 300' : layout === 'canvas' ? `${canvasWidth} × ${canvasHeight}` : '440 × 660';
  const selected = sources[selectedSource] || DEFAULT_CANVAS_SOURCES[selectedSource];
  const selectedDraft = sourceDrafts[selectedSource] || sourceDraft(selected);
  const selectedLabel = MODULES.find(({ key }) => key === selectedSource)?.label || selectedSource;

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
      data-tour-id="onboarding-overlay"
      data-tour-ready="true"
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
          Dein Stream, dein Look. Wähle Farben und Inhalte – die Vorschau zeigt deine echten Spielwerte.
          Voraussetzung: ein über den Discord verknüpfter Steam-Account.
        </p>
      </div>

      <div className="grid gap-5 xl:grid-cols-[minmax(0,1fr)_minmax(440px,0.9fr)]">
        <div className="space-y-5">
          {/* Stil, Layout & Spielmodus */}
          <div className="grid gap-3 sm:grid-cols-3">
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

            <div className="space-y-2">
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

          <fieldset className="space-y-3">
            <legend className="text-sm font-semibold text-white">Deine Farben</legend>
            <div className="flex flex-wrap gap-2">
              {[
                { name: 'Nachtgold', accent: '#d6b56c', background: '#0d0f14', text: '#f4f7fb' },
                { name: 'Eisblau', accent: '#67d8f3', background: '#101922', text: '#f3faff' },
                { name: 'Flieder', accent: '#bca1ff', background: '#181322', text: '#f8f3ff' },
                { name: 'Papier', accent: '#765321', background: '#f5f3ef', text: '#17191f' },
              ].map(preset => <button key={preset.name} type="button" onClick={() => { setAccent(preset.accent); setBackground(preset.background); setText(preset.text); setTheme(preset.name === 'Papier' ? 'light' : 'dark'); }} className="inline-flex min-h-11 items-center gap-2 rounded-lg border border-border bg-background/60 px-3 text-sm text-white hover:border-primary focus-visible:outline-2 focus-visible:outline-primary"><span className="h-3 w-3 rounded-full" style={{ background: preset.accent }} />{preset.name}</button>)}
            </div>
            <div className="grid gap-3 sm:grid-cols-3">
              {[{ key: 'accent', label: 'Akzent', value: accent, change: setAccent }, { key: 'background', label: 'Hintergrund', value: background, change: setBackground }, { key: 'text', label: 'Schrift', value: text, change: setText }].map(color => <label key={color.key} className="flex min-h-16 items-center gap-3 rounded-xl border border-border bg-background/60 p-3"><input aria-label={color.label} type="color" value={color.value} onChange={event => color.change(event.target.value)} className="h-9 w-10 cursor-pointer border-0 bg-transparent" /><span className="text-sm text-white">{color.label}<span className="block font-mono text-xs text-text-secondary">{color.value.toUpperCase()}</span></span></label>)}
            </div>
            <label className="block text-sm text-white" htmlFor="overlay-radius">Rundung <span className="text-text-secondary">{radius} px</span><input id="overlay-radius" type="range" min={0} max={32} value={radius} onChange={event => setRadius(Number(event.target.value))} className="mt-2 block w-full accent-primary" /></label>
          </fieldset>

          {/* Module */}
          <fieldset className="space-y-3">
            <legend className="text-sm font-semibold text-white">Inhalte</legend>
            <div className="grid gap-2.5 sm:grid-cols-2">
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
          <div className="grid gap-4 sm:grid-cols-2">
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

          {layout === 'canvas' && (
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

              <div className="grid gap-3 sm:grid-cols-2">
                <label className="text-sm font-medium text-white">
                  Leinwand-Breite
                  <input type="number" min={320} max={3840} step={1} value={canvasWidthDraft} onFocus={() => setEditingCanvasDimension('width')} onChange={(event) => setCanvasWidthDraft(event.target.value)} onBlur={() => commitCanvasDimension('width')} className="mt-1 block w-full rounded-lg border border-border bg-background/70 px-3 py-2 font-mono text-sm text-white" />
                </label>
                <label className="text-sm font-medium text-white">
                  Leinwand-Höhe
                  <input type="number" min={180} max={2160} step={1} value={canvasHeightDraft} onFocus={() => setEditingCanvasDimension('height')} onChange={(event) => setCanvasHeightDraft(event.target.value)} onBlur={() => commitCanvasDimension('height')} className="mt-1 block w-full rounded-lg border border-border bg-background/70 px-3 py-2 font-mono text-sm text-white" />
                </label>
              </div>

              <div className="rounded-lg border border-border bg-background/60 p-3">
                <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
                  <div>
                    <h3 className="text-sm font-semibold text-white">Ausgewählte Quelle: {selectedLabel}</h3>
                    <p className="text-xs text-text-secondary">X/Y = Position · Breite/Höhe = Größe</p>
                  </div>
                  <button type="button" onClick={() => resetSource(selectedSource)} className="min-h-10 rounded-lg border border-border px-3 text-xs font-semibold text-text-secondary hover:border-primary hover:text-primary">Quelle zurücksetzen</button>
                </div>
                <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
                  {(['x', 'y', 'width', 'height'] as const).map((field) => (
                    <label key={field} className="text-xs font-semibold uppercase tracking-wider text-text-secondary">
                      {field === 'width' ? 'Breite' : field === 'height' ? 'Höhe' : field.toUpperCase()}
                      <input type="number" min={0} step={1} value={selectedDraft[field]} onFocus={() => setEditingField({ key: selectedSource, field })} onChange={(event) => setSourceDrafts(current => ({ ...current, [selectedSource]: { ...current[selectedSource], [field]: event.target.value } }))} onBlur={() => commitSourceField(selectedSource, field)} className="mt-1 block w-full rounded-lg border border-border bg-background px-2 py-2 font-mono text-sm font-normal normal-case tracking-normal text-white" />
                    </label>
                  ))}
                </div>
              </div>

              <div>
                <div className="mb-2 flex items-center justify-between gap-2">
                  <h3 className="text-sm font-semibold text-white">Quellen</h3>
                  <button type="button" onClick={resetCanvas} className="text-xs font-semibold text-text-secondary underline underline-offset-4 hover:text-primary">Leinwand zurücksetzen</button>
                </div>
                <div className="grid gap-2 sm:grid-cols-2">
                  {MODULES.map(({ key, label }) => {
                    const source = sources[key];
                    return (
                      <button
                        key={key}
                        type="button"
                        aria-pressed={selectedSource === key}
                        onClick={() => setSelectedSource(key)}
                        className={`flex min-h-11 items-center justify-between gap-3 rounded-lg border px-3 text-left text-sm transition-colors ${selectedSource === key ? 'border-primary bg-primary/10 text-white' : 'border-border bg-background/50 text-text-secondary hover:border-primary/60'}`}
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
            <div className="flex flex-col gap-2 sm:flex-row">
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
          {layout === 'canvas' && (
            <div className="flex flex-wrap items-center gap-3 rounded-lg border border-primary/25 bg-primary/5 px-3 py-2">
              <button type="button" onClick={saveLayout} className="min-h-11 rounded-lg border border-primary/50 bg-primary/15 px-4 text-sm font-semibold text-primary transition-colors hover:bg-primary/25">
                {saved ? 'Layout gespeichert' : 'Layout speichern'}
              </button>
              <span className="text-xs text-text-secondary">Dein Layout wird in diesem Browser gemerkt. Die fertige URL kannst du danach in OBS einfügen.</span>
            </div>
          )}
          {saveFailed && <p role="alert" className="text-sm text-warning">Das Layout konnte in diesem Browser nicht gespeichert werden.</p>}
          {unsaved && <button type="button" onClick={() => setRetainedUrl(overlayUrl)} className="min-h-11 text-sm text-primary underline underline-offset-4">Adresse in OBS übernommen</button>}

          {/* OBS */}
          <div className="space-y-2">
            <h3 className="text-sm font-semibold text-white">So fügst du es in OBS ein</h3>
            <ol className="list-decimal space-y-1.5 pl-5 text-sm text-text-secondary">
              <li>Klick in OBS unten bei „Quellen" auf das Plus und wähle „Browser".</li>
              <li>Vergib einen Namen (z. B. „Deadlock-Stats") und bestätige mit OK.</li>
              <li>Füge die obige Overlay-URL in das Feld „URL" ein.</li>
              <li>{layout === 'canvas' ? `Stell die Browser-Quelle auf ${canvasWidth} × ${canvasHeight}, passend zur Leinwand unten.` : 'Stell die Größe der Browser-Quelle passend zum Layout ein (siehe Empfehlung unten).'}</li>
              <li>Zieh die Quelle an die gewünschte Stelle — sie aktualisiert sich automatisch.</li>
            </ol>
            <div className="flex items-center justify-between gap-3 rounded-lg border border-border bg-background/60 px-3 py-2 text-sm">
              <span className="font-medium text-text-secondary">Empfohlene OBS-Größe</span>
              <span className="font-mono font-semibold text-white">{recommendedSize}</span>
            </div>
          </div>
        </div>

        {/* Vorschau */}
        <div ref={previewHostRef} className="space-y-3 xl:sticky xl:top-6 xl:self-start">
          <div className="flex flex-wrap items-center justify-between gap-2"><h3 className="text-sm font-semibold text-white">So sieht es im Stream aus</h3><select aria-label="Vorschau-Hintergrund" value={previewBackdrop} onChange={event => setPreviewBackdrop(event.target.value as typeof previewBackdrop)} className="min-h-11 rounded-lg border border-border bg-background px-2 text-xs text-white"><option value="checker">Transparenz</option><option value="dark">Dunkle Szene</option><option value="light">Helle Szene</option></select></div>
          <div
            ref={editorRef}
            className={`relative overflow-hidden rounded-xl border border-border bg-background/60 ${layout === 'canvas' ? '' : 'overflow-x-auto'}`}
            style={{
              ...(previewBackdrop === 'checker' ? CHECKER_STYLE : { background: previewBackdrop === 'dark' ? '#090a0d' : '#d8dce1' }),
              ...(layout === 'canvas' ? { width: `${canvasPreviewWidth}px`, height: `${canvasPreviewHeight}px`, marginInline: 'auto' } : {}),
            }}
          >
            <iframe
              src={debouncedUrl}
              title="Overlay mit deinen Spielwerten"
              style={layout === 'canvas' ? { height: '100%' } : { height: `${previewHeight}px` }}
              className={layout === 'canvas' ? 'absolute inset-0 block h-full w-full min-w-0 border-0 bg-transparent' : 'block min-w-[440px] w-full border-0 bg-transparent'}
            />
            {layout === 'canvas' && (
              <div className="absolute inset-0 z-10">
                {MODULES.map(({ key, label }) => {
                  if (!modules[key]) return null;
                  const source = sources[key];
                  return (
                    <button
                      key={key}
                      type="button"
                      aria-label={`${label} verschieben und auswählen`}
                      title={`${label}: ziehen zum Verschieben`}
                      onPointerDown={(event) => beginSourceDrag(event, key, 'move')}
                      className={`absolute box-border cursor-move rounded-lg border-2 text-left transition-colors ${selectedSource === key ? 'border-primary bg-primary/10' : 'border-primary/30 bg-primary/5 hover:border-primary/70'}`}
                      style={{ left: `${source.x / canvasWidth * 100}%`, top: `${source.y / canvasHeight * 100}%`, width: `${source.width / canvasWidth * 100}%`, height: `${source.height / canvasHeight * 100}%` }}
                    >
                      <span className="pointer-events-none absolute left-1 top-1 rounded bg-background/85 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-primary">{label}</span>
                      <span
                        aria-hidden="true"
                        onPointerDown={(event) => beginSourceDrag(event, key, 'resize')}
                        className="absolute bottom-0 right-0 h-4 w-4 cursor-se-resize rounded-tl border-l border-t border-primary bg-primary/80"
                        title={`${label}: ziehen zum Vergrößern oder Verkleinern`}
                      />
                    </button>
                  );
                })}
              </div>
            )}
          </div>
          <p className="text-xs leading-relaxed text-text-secondary">{layout === 'canvas' ? 'Zieh eine Quelle in der Vorschau oder nutze die X-/Y-/Breite-/Höhe-Felder. Speichere danach das Layout und kopiere die fertige URL für OBS.' : 'Keine Werte sichtbar? Verknüpfe dein Steam-Konto im Discord. Die Vorschau verwendet denselben Datenstand wie OBS. Nach Änderungen die neue Adresse in OBS einsetzen.'}</p>
        </div>
      </div>
    </motion.section>
  );
}
