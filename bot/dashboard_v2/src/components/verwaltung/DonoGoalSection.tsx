import { useEffect, useMemo, useState, type FormEvent } from 'react';
import { Check, Copy, Gem, Loader2, PlugZap, RotateCcw } from 'lucide-react';
import { useLanguage } from '@/context/LanguageContext';
import { fetchDonoGoal, resetDonoGoal, saveDonoGoal, testDonoGoal, type DonoGoalSettings } from '@/api/donoGoal';
import { SectionCard } from './ui';

const OVERLAY_WIDTH = 900;
const OVERLAY_HEIGHT = 150;
const CURRENCIES = ['EUR', 'USD', 'GBP', 'CHF'];

interface DonoGoalForm {
  title: string;
  goal: string;
  currency: string;
  channelId: string;
  jwt: string;
}

type Busy = 'save' | 'disconnect' | 'reset' | 'test' | null;
type Notice = { tone: 'ok' | 'error'; text: string } | null;

const TEST_MESSAGES: Record<string, string> = {
  not_configured: 'Trag zuerst Kanal-ID und JWT ein und speichere.',
  unauthorized: 'StreamElements lehnt den JWT ab. Kopier ihn neu aus deinem StreamElements-Konto.',
  unreachable: 'StreamElements ist gerade nicht erreichbar. Versuch es gleich nochmal.',
};

function toForm(settings: DonoGoalSettings): DonoGoalForm {
  return {
    title: settings.title,
    goal: String(settings.goal),
    currency: settings.currency || 'EUR',
    channelId: settings.se_channel_id,
    jwt: '',
  };
}

function parseGoal(raw: string): number | null {
  const value = Number(raw.replace(',', '.'));
  return Number.isFinite(value) && value > 0 && value <= 10_000_000 ? value : null;
}

function useScale(width: number) {
  const [node, setNode] = useState<HTMLDivElement | null>(null);
  const [scale, setScale] = useState(1);
  useEffect(() => {
    if (!node || typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(entries => {
      const available = entries[0]?.contentRect.width ?? width;
      setScale(Math.min(1, available / width));
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, [node, width]);
  return { ref: setNode, scale };
}

const inputClass = 'mt-1 block w-full rounded-lg border border-border bg-background px-3 py-2 text-sm text-white placeholder:text-text-secondary/60 focus:border-primary focus:outline-none';
const labelClass = 'block text-xs font-semibold uppercase tracking-wider text-text-secondary';
const buttonClass = 'inline-flex items-center justify-center gap-2 rounded-lg border border-border bg-card px-4 py-2 text-sm font-semibold text-white transition-colors hover:border-border-hover hover:bg-card-hover disabled:cursor-not-allowed disabled:opacity-50';

export function DonoGoalSection({ login }: { login: string }) {
  const { t, locale } = useLanguage();
  const [settings, setSettings] = useState<DonoGoalSettings | null>(null);
  const [form, setForm] = useState<DonoGoalForm | null>(null);
  const [loadError, setLoadError] = useState(false);
  const [busy, setBusy] = useState<Busy>(null);
  const [notice, setNotice] = useState<Notice>(null);
  const [copied, setCopied] = useState(false);
  const [previewKey, setPreviewKey] = useState(0);
  const { ref: previewRef, scale } = useScale(OVERLAY_WIDTH);

  useEffect(() => {
    let active = true;
    fetchDonoGoal()
      .then(value => { if (active) { setSettings(value); setForm(toForm(value)); } })
      .catch(() => { if (active) setLoadError(true); });
    return () => { active = false; };
  }, []);

  const overlayUrl = useMemo(() => {
    const origin = typeof window === 'undefined' ? '' : window.location.origin;
    return login ? `${origin}/twitch/dono-goal?streamer=${encodeURIComponent(login)}` : '';
  }, [login]);
  const previewUrl = overlayUrl && settings?.configured ? overlayUrl : `${overlayUrl || '/twitch/dono-goal?'}&demo=1`;

  function applySettings(next: DonoGoalSettings, text: string) {
    setSettings(next);
    setForm(toForm(next));
    setPreviewKey(key => key + 1);
    setNotice({ tone: 'ok', text });
  }

  async function run(kind: Exclude<Busy, null>, action: () => Promise<void>) {
    if (busy) return;
    setBusy(kind);
    setNotice(null);
    try { await action(); }
    catch { setNotice({ tone: 'error', text: 'Speichern fehlgeschlagen. Bitte prüf deine Eingaben und versuch es nochmal.' }); }
    finally { setBusy(null); }
  }

  function submit(event: FormEvent) {
    event.preventDefault();
    if (!form) return;
    const goal = parseGoal(form.goal);
    if (goal === null) {
      setNotice({ tone: 'error', text: 'Bitte gib ein Ziel größer als 0 ein.' });
      return;
    }
    void run('save', async () => {
      const next = await saveDonoGoal({
        title: form.title,
        goal,
        currency: form.currency,
        se_channel_id: form.channelId.trim(),
        se_jwt: form.jwt.trim() || undefined,
      });
      applySettings(next, 'Gespeichert.');
    });
  }

  function disconnect() {
    if (!form || !settings) return;
    void run('disconnect', async () => {
      const next = await saveDonoGoal({
        title: settings.title,
        goal: settings.goal,
        currency: settings.currency,
        se_channel_id: settings.se_channel_id,
        clear_jwt: true,
      });
      applySettings(next, 'StreamElements getrennt.');
    });
  }

  function reset() {
    if (!window.confirm(t('Zählstand jetzt auf 0 setzen? Ab sofort zählen nur neue Spenden.'))) return;
    void run('reset', async () => {
      const next = await resetDonoGoal();
      applySettings(next, 'Zählstand zurückgesetzt. Ab jetzt zählen nur neue Spenden.');
    });
  }

  function test() {
    if (busy) return;
    setBusy('test');
    setNotice(null);
    testDonoGoal()
      .then(result => setNotice(result.ok
        ? { tone: 'ok', text: 'Verbindung klappt. Deine Spenden werden gezählt.' }
        : { tone: 'error', text: TEST_MESSAGES[result.error ?? 'unreachable'] ?? TEST_MESSAGES.unreachable }))
      .catch(() => setNotice({ tone: 'error', text: TEST_MESSAGES.unreachable }))
      .finally(() => setBusy(null));
  }

  async function copyUrl() {
    if (!overlayUrl) return;
    try {
      await navigator.clipboard.writeText(overlayUrl);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    } catch {
      setNotice({ tone: 'error', text: 'Kopieren hat nicht geklappt. Markier die URL und kopier sie von Hand.' });
    }
  }

  const update = (patch: Partial<DonoGoalForm>) => setForm(current => (current ? { ...current, ...patch } : current));
  const countFrom = settings?.count_from ? new Date(settings.count_from) : null;

  return (
    <SectionCard
      className="mt-6"
      eyebrow={t('Spendenziel')}
      icon={<Gem className="h-4 w-4" />}
      title={t('Spendenziel-Overlay')}
      description={t('Zeigt dein Spendenziel aus StreamElements als Balken in OBS. Neue Spenden erscheinen nach spätestens 25 Sekunden.')}
    >
      {!form && !loadError && <p role="status" className="flex items-center gap-2 text-sm text-text-secondary"><Loader2 className="h-4 w-4 animate-spin" />{t('Einstellungen werden geladen …')}</p>}
      {loadError && <p role="alert" className="text-sm text-danger">{t('Einstellungen konnten nicht geladen werden. Bitte Seite neu laden.')}</p>}
      {form && settings && (
        <div className="grid gap-6 xl:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
          <form onSubmit={submit} className="space-y-4">
            <label className={labelClass}>
              {t('Titel')}
              <input className={inputClass} value={form.title} maxLength={80} placeholder={t('Neues Setup für den Stream')} onChange={event => update({ title: event.target.value })} />
            </label>
            <div className="grid grid-cols-[minmax(0,1fr)_7rem] gap-3">
              <label className={labelClass}>
                {t('Ziel')}
                <input className={inputClass} inputMode="decimal" value={form.goal} onChange={event => update({ goal: event.target.value })} />
              </label>
              <label className={labelClass}>
                {t('Währung')}
                <select className={inputClass} value={form.currency} onChange={event => update({ currency: event.target.value })}>
                  {CURRENCIES.map(code => <option key={code} value={code}>{code}</option>)}
                </select>
              </label>
            </div>
            <div className="rounded-xl border border-border bg-background/60 p-4">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <p className="text-sm font-semibold text-white">StreamElements</p>
                <span className={`rounded-full px-2.5 py-0.5 text-xs font-semibold ${settings.connected ? 'bg-success/15 text-success' : 'bg-white/5 text-text-secondary'}`}>
                  {settings.connected ? t('verbunden') : t('nicht verbunden')}
                </span>
              </div>
              <p className="mt-1 text-xs text-text-secondary">{t('Kanal-ID und JWT findest du bei StreamElements unter Konto, Kanäle. Der JWT wird verschlüsselt gespeichert und nie wieder angezeigt.')}</p>
              <label className={`${labelClass} mt-3`}>
                {t('Kanal-ID')}
                <input className={`${inputClass} font-mono`} value={form.channelId} maxLength={64} autoComplete="off" spellCheck={false} onChange={event => update({ channelId: event.target.value })} />
              </label>
              <label className={`${labelClass} mt-3`}>
                {t('JWT-Token')}
                <input className={`${inputClass} font-mono`} type="password" value={form.jwt} maxLength={4096} autoComplete="off" spellCheck={false} placeholder={settings.connected ? t('Gespeichert. Nur ausfüllen, um ihn zu ersetzen.') : t('JWT aus StreamElements einfügen')} onChange={event => update({ jwt: event.target.value })} />
              </label>
              <div className="mt-3 flex flex-wrap gap-2">
                <button type="button" className={buttonClass} disabled={busy !== null} onClick={test}>
                  {busy === 'test' ? <Loader2 className="h-4 w-4 animate-spin" /> : <PlugZap className="h-4 w-4" />}
                  {t('Verbindung testen')}
                </button>
                {settings.connected && (
                  <button type="button" className={buttonClass} disabled={busy !== null} onClick={disconnect}>
                    {t('Trennen')}
                  </button>
                )}
              </div>
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <button type="submit" disabled={busy !== null} className="inline-flex items-center gap-2 rounded-lg bg-primary px-4 py-2 text-sm font-semibold text-background transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50">
                {busy === 'save' && <Loader2 className="h-4 w-4 animate-spin" />}
                {t('Speichern')}
              </button>
              <button type="button" className={buttonClass} disabled={busy !== null || !settings.configured} onClick={reset}>
                {busy === 'reset' ? <Loader2 className="h-4 w-4 animate-spin" /> : <RotateCcw className="h-4 w-4" />}
                {t('Zählstand zurücksetzen')}
              </button>
            </div>
            {countFrom && <p className="text-xs text-text-secondary">{t('Gezählt werden Spenden seit')} {countFrom.toLocaleString(locale, { dateStyle: 'medium', timeStyle: 'short' })}.</p>}
            {notice && <p role={notice.tone === 'error' ? 'alert' : 'status'} className={`text-sm ${notice.tone === 'error' ? 'text-danger' : 'text-success'}`}>{t(notice.text)}</p>}
          </form>
          <div className="min-w-0 space-y-4">
            <div>
              <p className={labelClass}>{t('Live-Vorschau')}</p>
              <div ref={previewRef} className="mt-2 overflow-hidden rounded-xl border border-border bg-[repeating-conic-gradient(#1b1f27_0%_25%,#141820_0%_50%)] bg-[length:24px_24px]" style={{ height: OVERLAY_HEIGHT * scale }}>
                <iframe
                  key={previewKey}
                  title={t('Spendenziel-Vorschau')}
                  src={previewUrl}
                  width={OVERLAY_WIDTH}
                  height={OVERLAY_HEIGHT}
                  className="block origin-top-left border-0"
                  style={{ transform: `scale(${scale})` }}
                />
              </div>
              {!settings.configured && <p className="mt-2 text-xs text-text-secondary">{t('Noch nicht gespeichert, die Vorschau zeigt Beispielspenden.')}</p>}
            </div>
            <div>
              <p className={labelClass}>{t('OBS-URL')}</p>
              {overlayUrl ? (
                <div className="mt-2 flex gap-2">
                  <input readOnly value={overlayUrl} onFocus={event => event.target.select()} className={`${inputClass} mt-0 font-mono text-xs`} aria-label={t('OBS-URL')} />
                  <button type="button" className={`${buttonClass} shrink-0`} onClick={() => void copyUrl()}>
                    {copied ? <Check className="h-4 w-4" /> : <Copy className="h-4 w-4" />}
                    {copied ? t('Kopiert') : t('Kopieren')}
                  </button>
                </div>
              ) : (
                <p className="mt-2 text-sm text-text-secondary">{t('Die OBS-URL erscheint, sobald dein Twitch-Konto verknüpft ist.')}</p>
              )}
              <p className="mt-2 text-xs text-text-secondary">{t('In OBS als Browser-Quelle hinzufügen, empfohlene Größe 900 × 150.')}</p>
            </div>
          </div>
        </div>
      )}
    </SectionCard>
  );
}
