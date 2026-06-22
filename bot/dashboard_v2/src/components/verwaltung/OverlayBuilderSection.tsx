import { useEffect, useMemo, useState, type CSSProperties } from 'react';
import { motion } from 'framer-motion';
import { Check, Copy } from 'lucide-react';

type OverlayFlag = 'rank' | 'winrate' | 'streak' | 'live';
type OverlayPosition = 'bl' | 'br' | 'tl' | 'tr';

const TOGGLES: Array<{ key: OverlayFlag; label: string }> = [
  { key: 'rank', label: 'Rang' },
  { key: 'winrate', label: 'Winrate' },
  { key: 'streak', label: 'Aktuelle Serie' },
  { key: 'live', label: 'Live-Match (Hero & Minute)' },
];

const POSITIONS: Array<{ value: OverlayPosition; label: string }> = [
  { value: 'bl', label: 'Unten links' },
  { value: 'br', label: 'Unten rechts' },
  { value: 'tl', label: 'Oben links' },
  { value: 'tr', label: 'Oben rechts' },
];

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
  const [flags, setFlags] = useState<Record<OverlayFlag, boolean>>({
    rank: true,
    winrate: true,
    streak: true,
    live: true,
  });
  const [position, setPosition] = useState<OverlayPosition>('bl');
  const [copied, setCopied] = useState(false);

  const overlayUrl = useMemo(() => {
    const origin = typeof window === 'undefined' ? '' : window.location.origin;
    const params = new URLSearchParams({
      streamer: normalizedLogin,
      rank: flags.rank ? '1' : '0',
      winrate: flags.winrate ? '1' : '0',
      streak: flags.streak ? '1' : '0',
      live: flags.live ? '1' : '0',
      pos: position,
    });

    return `${origin}/twitch/overlay?${params.toString()}`;
  }, [flags.live, flags.rank, flags.streak, flags.winrate, normalizedLogin, position]);

  useEffect(() => {
    setCopied(false);
  }, [overlayUrl]);

  const toggleFlag = (key: OverlayFlag) => {
    setFlags((current) => ({ ...current, [key]: !current[key] }));
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
          Wähl aus, was dein Overlay zeigen soll, kopier die URL und füg sie in OBS als Browser-Quelle ein. Voraussetzung: ein über den Discord verknüpfter Steam-Account.
        </p>
      </div>

      <div className="grid gap-5 lg:grid-cols-[minmax(0,1fr)_minmax(300px,360px)]">
        <div className="space-y-5">
          <div className="grid gap-3 sm:grid-cols-2">
            {TOGGLES.map(({ key, label }) => {
              const enabled = flags[key];
              return (
                <div
                  key={key}
                  className="soft-elevate flex items-center justify-between gap-4 rounded-xl border border-border bg-background/60 p-4"
                >
                  <span className="text-sm font-semibold text-white">{label}</span>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={enabled}
                    aria-label={label}
                    onClick={() => toggleFlag(key)}
                    className={`relative inline-flex h-7 w-12 shrink-0 items-center rounded-full transition-colors ${
                      enabled ? 'bg-primary' : 'bg-border'
                    }`}
                  >
                    <span
                      className={`inline-block h-5 w-5 transform rounded-full bg-white transition-transform ${
                        enabled ? 'translate-x-6' : 'translate-x-1'
                      }`}
                    />
                  </button>
                </div>
              );
            })}
          </div>

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

          <div className="space-y-2">
            <label
              htmlFor="overlay-url"
              className="block text-sm font-semibold text-white"
            >
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

          <div className="space-y-2">
            <h3 className="text-sm font-semibold text-white">So fügst du es in OBS ein</h3>
            <ol className="list-decimal space-y-1.5 pl-5 text-sm text-text-secondary">
              <li>Klick in OBS unten bei „Quellen" auf das Plus und wähle „Browser".</li>
              <li>Vergib einen Namen (z. B. „Deadlock-Stats") und bestätige mit OK.</li>
              <li>Füge die obige Overlay-URL in das Feld „URL" ein.</li>
              <li>Setz Breite auf 360 und Höhe auf 200 und bestätige mit OK.</li>
              <li>Zieh die Quelle an die gewünschte Stelle in deiner Szene — fertig, sie aktualisiert sich automatisch.</li>
            </ol>
          </div>
        </div>

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
              className="block h-[220px] w-full border-0 bg-transparent"
            />
          </div>
        </div>
      </div>
    </motion.section>
  );
}
