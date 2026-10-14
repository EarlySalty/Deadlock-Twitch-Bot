import { useT } from '../../context/LanguageContext';
import { useState } from 'react';
import { motion } from 'framer-motion';
import { AlertTriangle, Loader2, PlugZap } from 'lucide-react';
import {
  disconnectBot,
  roleLabel,
  roleNeedsAttention,
  unmodNeedsAttention,
  type DisconnectBotResponse,
} from '@/api/disconnectBot';

type Stage = 'idle' | 'warn' | 'type';

interface Props {
  /** Eigener Twitch-Login aus der Session; leer → Aktion bleibt gesperrt. */
  login: string;
}

export function DisconnectBotSection({ login }: Props) {
  const t = useT();
  const [stage, setStage] = useState<Stage>('idle');
  const [confirm, setConfirm] = useState('');
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [report, setReport] = useState<DisconnectBotResponse | null>(null);

  const expected = login.trim().toLowerCase();
  const matches = confirm.trim().toLowerCase() === expected && expected.length > 0;

  const reset = () => {
    setStage('idle');
    setConfirm('');
    setError(null);
  };

  const run = async () => {
    setPending(true);
    setError(null);
    try {
      const result = await disconnectBot(confirm.trim());
      setReport(result);
      setStage('idle');
      setConfirm('');
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Trennung fehlgeschlagen — nichts geändert.');
    } finally {
      setPending(false);
    }
  };

  return (
    <motion.section
      className="panel-card rounded-2xl p-5 md:p-6 border border-danger/30"
      initial={{ opacity: 0, y: 16 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true }}
      transition={{ duration: 0.32, delay: 0.2 }}
    >
      <div className="mb-5">
        <p className="text-sm uppercase tracking-wider font-medium text-danger mb-1">{t("Kanal")}</p>
        <h2 className="display-font text-2xl font-bold text-white mb-1">{t("Bot vom Kanal trennen")}</h2>
        <p className="text-sm text-text-secondary">
          {t("Der Bot verlässt deinen Kanal: er gibt seine Moderator-Rechte ab, die Partnerschaft endet und er kommt nicht von selbst zurück. Deine Twitch-Verbindung bleibt bestehen — meldest du dich später wieder an, moddet er sich erneut selbst.")}
        </p>
      </div>

      {error && (
        <div className="mb-4 rounded-lg border border-danger/40 bg-danger/10 px-3 py-2 text-sm text-danger">
          {t(error)}
        </div>
      )}

      {report && (
        <div
          className={`mb-4 rounded-lg border px-3 py-3 text-sm ${
            unmodNeedsAttention(report.unmod) || roleNeedsAttention(report.discord_role)
              ? 'border-warning/40 bg-warning/10 text-warning'
              : 'border-success/40 bg-success/10 text-success'
          }`}
        >
          <p className="font-semibold">{t(report.message)}</p>
          <ul className="mt-2 space-y-1 text-xs text-text-secondary">
            <li>
              {t("Moderator-Rechte:")}{' '}
              {report.unmod === 'removed'
                ? t("entzogen")
                : report.unmod === 'not_moderator'
                  ? t("war ohnehin kein Moderator")
                  : t("ACHTUNG — bleiben bestehen ({outcome}). Bitte im Twitch-Chat mit /unmod entfernen.", { outcome: report.unmod })}
            </li>
            <li>{t("Partnerschaft:")} {report.departnered ? t("beendet") : t("war nicht mehr aktiv")}</li>
            <li>Opt-out: {report.opt_out ? t("gesetzt") : t("ACHTUNG — nicht gesetzt")}</li>
            <li>{t("Discord-Streamer-Rolle:")} {roleLabel(report.discord_role, t)}</li>
          </ul>
        </div>
      )}

      {stage === 'idle' && (
        <button
          type="button"
          disabled={!expected}
          onClick={() => {
            setReport(null);
            setStage('warn');
          }}
          className="inline-flex items-center gap-2 rounded-lg border border-danger/40 bg-danger/10 px-5 py-2.5 text-sm font-semibold text-danger transition-colors hover:bg-danger/20 disabled:opacity-50 disabled:cursor-not-allowed"
        >
          <PlugZap className="h-4 w-4" />
          {t("Bot vom Kanal trennen")}
        </button>
      )}

      {stage === 'warn' && (
        <div className="soft-elevate rounded-xl border border-danger/40 bg-background/60 p-4">
          <p className="flex items-start gap-2 text-sm text-white">
            <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-danger" />
            <span>
              {t("Danach laufen keine Chat-Funktionen, Raids, Statistiken oder Overlays mehr auf deinem Kanal. Bereits erfasste Daten bleiben erhalten.")}
            </span>
          </p>
          <div className="mt-4 flex flex-wrap gap-3">
            <button
              type="button"
              onClick={() => setStage('type')}
              className="inline-flex items-center gap-2 rounded-lg border border-danger/40 bg-danger/10 px-4 py-2 text-sm font-semibold text-danger transition-colors hover:bg-danger/20"
            >
              {t("Verstanden, weiter")}
            </button>
            <button
              type="button"
              onClick={reset}
              className="rounded-lg border border-border px-4 py-2 text-sm font-semibold text-text-secondary transition-colors hover:text-white"
            >
              {t("Abbrechen")}
            </button>
          </div>
        </div>
      )}

      {stage === 'type' && (
        <div className="soft-elevate rounded-xl border border-danger/40 bg-background/60 p-4">
          <label className="block text-sm text-text-secondary" htmlFor="disconnect-confirm">
            {t("Tippe zur Bestätigung")} <span className="font-semibold text-white">{expected}</span> {t("ein:")}
          </label>
          <input
            id="disconnect-confirm"
            value={confirm}
            onChange={(e) => setConfirm(e.target.value)}
            autoComplete="off"
            className="mt-2 w-full rounded-lg border border-border bg-background/80 px-3 py-2 text-sm text-white outline-none focus:border-danger/60"
            placeholder={expected}
          />
          <div className="mt-4 flex flex-wrap gap-3">
            <button
              type="button"
              disabled={!matches || pending}
              onClick={() => void run()}
              className="inline-flex items-center gap-2 rounded-lg border border-danger/40 bg-danger/10 px-4 py-2 text-sm font-semibold text-danger transition-colors hover:bg-danger/20 disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {pending ? <Loader2 className="h-4 w-4 animate-spin" /> : <PlugZap className="h-4 w-4" />}
              {t("Jetzt trennen")}
            </button>
            <button
              type="button"
              disabled={pending}
              onClick={reset}
              className="rounded-lg border border-border px-4 py-2 text-sm font-semibold text-text-secondary transition-colors hover:text-white disabled:opacity-50"
            >
              {t("Abbrechen")}
            </button>
          </div>
        </div>
      )}
    </motion.section>
  );
}
