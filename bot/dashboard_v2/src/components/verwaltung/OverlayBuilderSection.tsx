import { useEffect, useMemo, useState, type CSSProperties } from 'react';
import { motion } from 'framer-motion';
import { Check, Copy } from 'lucide-react';

type OverlayTheme = 'dark' | 'light' | 'accent';
type OverlayLayout = 'box' | 'bar';
type OverlayPosition = 'bl' | 'br' | 'tl' | 'tr';
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
  { value: 'dark', label: 'Dunkel' },
  { value: 'light', label: 'Hell' },
  { value: 'accent', label: 'Akzent' },
];

const LAYOUTS: Array<{ value: OverlayLayout; label: string }> = [
  { value: 'box', label: 'Box (Karte)' },
  { value: 'bar', label: 'Leiste' },
];

const POSITIONS: Array<{ value: OverlayPosition; label: string }> = [
  { value: 'bl', label: 'Unten links' },
  { value: 'br', label: 'Unten rechts' },
  { value: 'tl', label: 'Oben links' },
  { value: 'tr', label: 'Oben rechts' },
];

const MODULES: Array<{ key: ModuleKey; label: string }> = [
  { key: 'header', label: 'Spielername & Live-Badge' },
  { key: 'rank', label: 'Rang & Abzeichen' },
  { key: 'winrate', label: 'Winrate (letzte Spiele)' },
  { key: 'today', label: 'Heute (Siege/Niederlagen)' },
  { key: 'streak', label: 'Aktuelle Serie' },
  { key: 'kd', label: 'K/D' },
  { key: 'lastmatch', label: 'Letztes Match' },
  { key: 'mostplayed', label: 'Meistgespielter Hero' },
  { key: 'recent', label: 'Match-Verlauf' },
  { key: 'live', label: 'Live-Match (Hero & Minute)' },
  { key: 'branding', label: 'Branding-Hinweis' },
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

const CHECKER_STYLE: CSSProperties = {
  backgroundColor: '#101319',
  backgroundImage:
    'linear-gradient(45deg, rgba(255,255,255,0.08) 25%, transparent 25%), linear-gradient(-45deg, rgba(255,255,255,0.08) 25%, transparent 25%), linear-gradient(45deg, transparent 75%, rgba(255,255,255,0.08) 75%), linear-gradient(-45deg, transparent 75%, rgba(255,255,255,0.08) 75%)',
  backgroundPosition: '0 0, 0 10px, 10px -10px, -10px 0',
  backgroundSize: '20px 20px',
};

type OverlayBuilderSectionProps = {
  login: string;
};

export function OverlayBuilderSection({ login }: OverlayBuilderSectionProps) {
  const normalizedLogin = login.trim();
  const [theme, setTheme] = useState<OverlayTheme>('dark');
  const [layout, setLayout] = useState<OverlayLayout>('box');
  const [position, setPosition] = useState<OverlayPosition>('bl');
  const [opacity, setOpacity] = useState<number>(85);
  const [recentN, setRecentN] = useState<number>(10);
  const [modules, setModules] = useState<Record<ModuleKey, boolean>>(DEFAULT_MODULES);
  const [copied, setCopied] = useState(false);

  const overlayUrl = useMemo(() => {
    const origin = typeof window === 'undefined' ? '' : window.location.origin;
    const params = new URLSearchParams();
    params.set('streamer', normalizedLogin);
    params.set('theme', theme);
    params.set('layout', layout);
    params.set('pos', position);
    params.set('opacity', String(opacity));
    params.set('recent_n', String(recentN));
    for (const { key } of MODULES) {
      params.set(key, modules[key] ? '1' : '0');
    }
    return `${origin}/twitch/overlay?${params.toString()}`;
  }, [normalizedLogin, theme, layout, position, opacity, recentN, modules]);

  useEffect(() => {
    setCopied(false);
  }, [overlayUrl]);

  const toggleModule = (key: ModuleKey) => {
    setModules((current) => ({ ...current, [key]: !current[key] }));
  };

  const copyUrl = async () => {
    try {
      await navigator.clipboard.writeText(overlayUrl);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1800);
    } catch {
      setCopied(false);
    }
  };

  const previewHeight = layout === 'bar' ? 120 : 280;
  const recommendedSize = layout === 'bar' ? '560 × 120' : '360 × 280';

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
          Wähl Stil und Inhalte, kopier die URL und füg sie in OBS als Browser-Quelle ein.
          Voraussetzung: ein über den Discord verknüpfter Steam-Account.
        </p>
      </div>

      <div className="grid gap-5 lg:grid-cols-[minmax(0,1fr)_minmax(300px,380px)]">
        <div className="space-y-5">
          {/* Stil & Layout */}
          <div className="grid gap-3 sm:grid-cols-2">
            <div className="space-y-2">
              <label htmlFor="overlay-theme" className="block text-sm font-semibold text-white">
                Stil
              </label>
              <select
                id="overlay-theme"
                value={theme}
                onChange={(event) => setTheme(event.target.value as OverlayTheme)}
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
          </div>

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
                        className={`inline-block h-4 w-4 transform rounded-full bg-white transition-transform ${
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

          {/* Position */}
          <fieldset className="space-y-3">
            <legend className="text-sm font-semibold text-white">Position im Stream</legend>
            <div className="grid gap-2 sm:grid-cols-4">
              {POSITIONS.map(({ value, label }) => {
                const selected = position === value;
                return (
                  <label
                    key={value}
                    className={`cursor-pointer rounded-lg border px-3 py-2 text-center text-sm font-semibold transition-colors ${
                      selected
                        ? 'border-primary bg-primary/15 text-primary'
                        : 'border-border bg-background/60 text-text-secondary hover:border-border-hover hover:text-white'
                    }`}
                  >
                    <input
                      type="radio"
                      name="overlay-position"
                      value={value}
                      checked={selected}
                      onChange={() => setPosition(value)}
                      className="sr-only"
                    />
                    {label}
                  </label>
                );
              })}
            </div>
          </fieldset>

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

          {/* OBS */}
          <div className="space-y-2">
            <h3 className="text-sm font-semibold text-white">So fügst du es in OBS ein</h3>
            <ol className="list-decimal space-y-1.5 pl-5 text-sm text-text-secondary">
              <li>Klick in OBS unten bei „Quellen" auf das Plus und wähle „Browser".</li>
              <li>Vergib einen Namen (z. B. „Deadlock-Stats") und bestätige mit OK.</li>
              <li>Füge die obige Overlay-URL in das Feld „URL" ein.</li>
              <li>Stell die Größe der Browser-Quelle passend zum Layout ein (siehe Empfehlung unten).</li>
              <li>Zieh die Quelle an die gewünschte Stelle — sie aktualisiert sich automatisch.</li>
            </ol>
            <div className="flex items-center justify-between gap-3 rounded-lg border border-border bg-background/60 px-3 py-2 text-sm">
              <span className="font-medium text-text-secondary">Empfohlene OBS-Größe</span>
              <span className="font-mono font-semibold text-white">{recommendedSize}</span>
            </div>
          </div>
        </div>

        {/* Vorschau */}
        <div className="space-y-2">
          <h3 className="text-sm font-semibold text-white">Vorschau</h3>
          <div
            className="overflow-hidden rounded-xl border border-border bg-background/60"
            style={CHECKER_STYLE}
          >
            <iframe
              key={overlayUrl}
              src={overlayUrl}
              title="Vorschau"
              style={{ height: `${previewHeight}px` }}
              className="block w-full border-0 bg-transparent"
            />
          </div>
        </div>
      </div>
    </motion.section>
  );
}
