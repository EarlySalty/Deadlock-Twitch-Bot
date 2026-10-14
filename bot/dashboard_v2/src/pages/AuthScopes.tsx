import { useLanguage, useT } from '../context/LanguageContext';
import {
  CircleHelp,
  ClipboardCopy,
  History,
  RefreshCw,
  ShieldCheck,
  ShieldOff,
  Sparkles,
} from 'lucide-react';
import { useMemo } from 'react';
import { useScopes } from '../hooks/useScopes';
import type { ScopeStatus } from '@/types/scopes';

const demoScopes = new Set<string>([
  'channel:manage:raids',
  'channel:manage:moderators',
  'channel:bot',
]);

function ScopeCard({ scope }: { scope: ScopeStatus }) {
  const t = useT();
  const isMissing = scope.status === 'missing';
  const isCritical = scope.importance === 'critical';
  const isNew = !!scope.addedAt;

  return (
    <div
      className={`relative overflow-visible rounded-xl border p-4 transition-colors ${
        isMissing
          ? 'border-danger/30 bg-danger/5'
          : 'border-border hover:border-border-hover bg-card/80'
      }`}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="space-y-1">
          <div className="flex flex-wrap items-center gap-2 text-xs uppercase tracking-wide text-text-secondary">
            <span>{t(scope.label)}</span>
            {isCritical && (
              <span className="rounded-full bg-danger/10 px-2 py-0.5 text-[10px] font-semibold text-danger">
                {t("Kritisch")}
              </span>
            )}
            {isNew && (
              <span className="rounded-full bg-accent/15 px-2 py-0.5 text-[10px] font-semibold text-accent">
                {t("Neu")}
              </span>
            )}
          </div>
          <div className="break-all text-sm font-semibold text-white">{scope.id}</div>
        </div>

        <div
          className={`flex items-center gap-1 rounded-lg px-2 py-1 text-xs font-semibold ${
            isMissing
              ? 'bg-danger/15 text-danger border border-danger/30'
              : 'bg-success/15 text-success border border-success/30'
          }`}
        >
          {isMissing ? <ShieldOff className="h-4 w-4" /> : <ShieldCheck className="h-4 w-4" />}
          {isMissing ? 'Fehlt' : t("Aktiv")}
        </div>
      </div>

      <p className="mt-2 text-sm text-text-secondary">{t(scope.description)}</p>

      <div className="mt-3 flex items-center gap-2 text-xs text-text-secondary">
        <div className="relative group">
          <CircleHelp className="h-4 w-4 text-text-secondary" />
          <div className="absolute left-0 top-6 z-20 w-72 rounded-lg border border-border bg-bg px-3 py-2 text-[12px] text-white opacity-0 shadow-xl transition-opacity duration-150 group-hover:opacity-100">
            {t(scope.why)}
          </div>
        </div>
        <span className="hidden sm:inline">{t("Warum:")}</span>
        <span className="line-clamp-1 sm:line-clamp-none">{t(scope.why)}</span>
      </div>
    </div>
  );
}

export function AuthScopes() {
  const t = useT();
  const { locale } = useLanguage();
  const formatDate = (raw: string) => locale === 'de-DE' ? raw : new Date(`${raw}T00:00:00`).toLocaleDateString(locale, { dateStyle: 'medium' });
  const { input, setInput, statuses, summary, changelog, setFromList, reset, scopeCatalog } =
    useScopes();

  const missingLabels = useMemo(
    () => summary.missing.map(scope => t(scope.label)).slice(0, 3),
    [summary.missing, t],
  );

  const allScopeIds = useMemo(() => scopeCatalog.map(scope => scope.id), [scopeCatalog]);

  return (
    <div className="space-y-6">
      {/* Hero / Summary */}
      <div className="rounded-2xl border border-border bg-card/70 p-6 shadow-lg">
        <div className="flex flex-col gap-4 md:flex-row md:items-center md:justify-between">
          <div className="space-y-2">
            <div className="flex items-center gap-2 text-xs uppercase tracking-wide text-text-secondary">
              <span>Scopes</span>
              <span className="rounded-full bg-white/5 px-2 py-0.5 text-[10px] text-text-secondary">
                {t("Frontend Preview")}
              </span>
            </div>
            <h2 className="text-2xl font-bold text-white">{t("OAuth Scope Checker")}</h2>
            <p className="max-w-3xl text-sm text-text-secondary">
              {t("Vergleicht die aktuell erteilten Broadcaster-/Streamer-Scopes mit unserer benötigten Liste. Bot-User-Scopes wie")} <code>user:read:chat</code> {t("laufen zentral über den Bot-Account und sind bewusst nicht Teil dieser Streamer-Matrix.")}
            </p>
          </div>

          <div className="flex items-center gap-3">
            <div className="rounded-xl border border-border bg-black/30 px-4 py-3 text-center">
              <div className="text-sm text-text-secondary">{t("Abdeckung")}</div>
              <div className="text-2xl font-semibold text-white">{summary.coverage.toLocaleString(locale)}%</div>
              <div className="mt-2 h-2 w-40 overflow-hidden rounded-full bg-white/5">
                <div
                  className="h-full rounded-full bg-gradient-to-r from-accent to-primary"
                  style={{ width: `${summary.coverage.toLocaleString(locale)}%` }}
                />
              </div>
            </div>
            <div className="rounded-xl border border-border bg-black/30 px-4 py-3 text-center">
              <div className="text-sm text-text-secondary">{t("Kritisch fehlen")}</div>
              <div className="text-2xl font-semibold text-danger">{summary.criticalMissing.length.toLocaleString(locale)}</div>
              <div className="text-xs text-text-secondary">{t("von")} {summary.total.toLocaleString(locale)}</div>
            </div>
          </div>
        </div>

        <div className="mt-4 grid grid-cols-1 gap-3 md:grid-cols-3">
          <div className="rounded-lg border border-border bg-black/20 px-4 py-3 text-sm text-text-secondary">
            <div className="mb-1 flex items-center gap-2 text-white">
              <Sparkles className="h-4 w-4 text-accent" />
              {t("Streamer-Scopes")}
            </div>
            {summary.missing.length === 0 ? (
              <span className="text-success">{t("Alle Scopes vorhanden ✔")}</span>
            ) : (
              <span>
                {t("Fehlt:")} {missingLabels.join(', ')}
                {summary.missing.length > missingLabels.length && ' …'}
              </span>
            )}
          </div>

          <div className="rounded-lg border border-border bg-black/20 px-4 py-3 text-sm text-text-secondary">
            <div className="mb-1 flex items-center gap-2 text-white">
              <History className="h-4 w-4 text-accent" />
              {t("Letzter Change")}
            </div>
            <span>{summary.lastChange ? formatDate(summary.lastChange) : t("—")}</span>
          </div>

          <div className="rounded-lg border border-border bg-black/20 px-4 py-3 text-sm text-text-secondary">
            <div className="mb-1 flex items-center gap-2 text-white">
              <RefreshCw className="h-4 w-4 text-accent" />
              {t("Bot-Scopes")}
            </div>
            <span className="text-xs text-text-secondary">
              {t("Zentral:")} <code>user:read:chat</code>, <code>user:write:chat</code>.
              <code>user:bot</code> {t("erst bei App-Token-Chat-EventSub.")}
            </span>
          </div>
        </div>
      </div>

      {/* Input area */}
      <div className="rounded-2xl border border-border bg-card/80 p-5 shadow">
        <div className="mb-3 flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
          <div>
            <h3 className="text-lg font-semibold text-white">{t("Aktuelle Streamer-Scopes")}</h3>
            <p className="text-sm text-text-secondary">
              {t("Füge hier die Broadcaster-Scopes des Streamer-Tokens ein (Komma oder Leerzeichen getrennt). Bot-Scopes werden getrennt zentral geprüft.")}
            </p>
          </div>
          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              onClick={() => setFromList(demoScopes)}
              className="flex items-center gap-2 rounded-lg border border-border px-3 py-2 text-sm text-text-secondary transition hover:border-border-hover"
            >
              <ClipboardCopy className="h-4 w-4" />
              {t("Demo füllen")}
            </button>
            <button
              type="button"
              onClick={() => setFromList(allScopeIds)}
              className="flex items-center gap-2 rounded-lg border border-border px-3 py-2 text-sm text-text-secondary transition hover:border-border-hover"
            >
              <ShieldCheck className="h-4 w-4 text-success" />
              {t("Alles autorisiert")}
            </button>
            <button
              type="button"
              onClick={reset}
              className="flex items-center gap-2 rounded-lg border border-border px-3 py-2 text-sm text-text-secondary transition hover:border-border-hover"
            >
              <RefreshCw className="h-4 w-4" />
              {t("Zurücksetzen")}
            </button>
          </div>
        </div>

        <textarea
          value={input}
          onChange={e => setInput(e.target.value)}
          rows={3}
          className="w-full rounded-xl border border-border bg-black/30 px-3 py-3 text-sm text-white outline-none ring-0 focus:border-accent focus:ring-2 focus:ring-accent/40"
          placeholder="channel:manage:raids channel:bot bits:read …"
        />
        <p className="mt-2 text-xs text-text-secondary">
          {t("Tipp: Du kannst Scopes auch per URL setzen (\"?scopes=channel:manage:raids,channel:bot\"), wir lesen sie beim Laden ein und speichern sie lokal.")} <code>user:read:chat</code> {t("oder")}
          <code>user:bot</code> {t("gehoeren nicht in diese Streamer-Liste.")}
        </p>
      </div>

      {/* Scope grid */}
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
        {statuses.map(scope => (
          <ScopeCard key={scope.id} scope={scope} />
        ))}
      </div>

      {/* Changelog */}
      <div className="rounded-2xl border border-border bg-card/80 p-5 shadow">
        <div className="mb-3 flex items-center justify-between">
          <div>
            <p className="text-xs uppercase tracking-wide text-text-secondary">What&apos;s new</p>
            <h3 className="text-lg font-semibold text-white">{t("Changelog (Scopes)")}</h3>
            <p className="text-sm text-text-secondary">
              {t("Platzhalter-Einträge – hier kannst du Updates posten, wenn Backend/Scopes sich ändern.")}
            </p>
          </div>
          <span className="rounded-full bg-white/5 px-3 py-1 text-xs text-text-secondary">
            {t("Sichtbar für Streamer")}
          </span>
        </div>

        <div className="space-y-3">
          {changelog.map(entry => (
            <div
              key={`${formatDate(entry.date)}-${entry.title}`}
              className="rounded-xl border border-border bg-black/20 p-4"
            >
              <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
                <div>
                  <p className="text-xs uppercase tracking-wide text-text-secondary">{formatDate(entry.date)}</p>
                  <h4 className="text-base font-semibold text-white">{t(entry.title)}</h4>
                </div>
                <div className="flex flex-wrap gap-2">
                  {entry.tags?.map(tag => (
                    <span
                      key={tag}
                      className="rounded-full bg-white/5 px-3 py-1 text-xs text-text-secondary"
                    >
                      {t(tag)}
                    </span>
                  ))}
                </div>
              </div>
              <ul className="mt-2 space-y-1 text-sm text-text-secondary">
                {entry.items.map(item => (
                  <li key={item} className="flex items-start gap-2">
                    <span className="mt-[6px] h-1.5 w-1.5 rounded-full bg-accent" />
                    <span>{t(item)}</span>
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
