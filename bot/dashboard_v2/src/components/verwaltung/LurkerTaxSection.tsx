import { useT } from '../../context/LanguageContext';
import { useEffect, useState } from 'react';
import { CheckCircle2, CircleAlert, Gift, Loader2 } from 'lucide-react';
import { CommandRow, SectionCard } from './ui';
import {
  fetchLurkerTaxSettings,
  toggleLurkerTax,
  type LurkerTaxSettingsResponse,
} from '@/api/lurkerTax';

export function LurkerTaxSection() {
  const t = useT();
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [savedAt, setSavedAt] = useState<number | null>(null);
  const [settings, setSettings] = useState<LurkerTaxSettingsResponse | null>(null);
  const [pending, setPending] = useState(false);

  const load = async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await fetchLurkerTaxSettings();
      setSettings(data);
    } catch {
      setError('Lurker-Steuer-Status konnte nicht geladen werden.');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void load();
  }, []);

  const onToggle = async () => {
    if (!settings || pending) return;
    const want = !settings.lurker_tax_enabled;
    setPending(true);
    setError(null);
    setSavedAt(null);
    try {
      const data = await toggleLurkerTax(want);
      setSettings((current) => ({
        lurker_tax_enabled: data.lurker_tax_enabled,
        has_moderator_read_chatters:
          current?.has_moderator_read_chatters ?? settings.has_moderator_read_chatters,
        reward_present: current?.reward_present ?? settings.reward_present,
      }));
      setSavedAt(Date.now());
    } catch {
      setError('Speichern fehlgeschlagen. Bitte versuch es nochmal.');
    } finally {
      setPending(false);
    }
  };

  const enabled = Boolean(settings?.lurker_tax_enabled);
  const scopeReady = Boolean(settings?.has_moderator_read_chatters);
  const rewardStatus: 'found' | 'missing' | 'unknown' =
    settings?.reward_present === true
      ? 'found'
      : settings?.reward_present === false
        ? 'missing'
        : 'unknown';

  return (
    <SectionCard
      eyebrow={t("Stammzuschauer")}
      title={t("Lurker-Steuer")}
      description={t("Erinnert deine ruhigsten Stamm-Lurker ab und zu mit einem freundlichen @-Hinweis daran, mal wieder Hallo zu sagen. Höchstens zwei pro Erinnerung und nur bei langjährigen Zuschauern. Standardmäßig aus.")}
    >
      {loading ? (
        <div className="flex items-center gap-3 text-text-secondary text-sm">
          <Loader2 className="h-4 w-4 animate-spin text-primary" />
          {t("Status wird geladen ...")}
        </div>
      ) : !settings ? (
        <p role="alert" className="text-sm text-text-secondary">
          {error ? t(error) : t("Die Einstellung ist gerade nicht verfügbar. Bitte lade die Seite neu.")}
        </p>
      ) : (
        <>
          <CommandRow
            title={t("Lurker-Steuer")}
            description={enabled
              ? t("Holt sehr sparsam deine ruhigsten Stamm-Lurker zurück in den Chat.")
              : t("Aktiviere sie, um stille Stammzuschauer sanft zurück in den Chat zu holen.")}
            inputSide="end"
            savedAt={savedAt}
            error={error}
            toggle={{ checked: enabled, onChange: () => void onToggle(), label: t("Lurker-Steuer"), pending }}
          />
          <RewardGuide status={rewardStatus} />
          {enabled && !scopeReady && (
            <p role="status" className="mt-3 text-sm text-warning">
              {t("Der Schalter ist an, aber der Community-Bot braucht dafür noch die Leseberechtigung für deine Zuschauerliste. Darum kümmern wir uns, du musst nichts tun.")}
            </p>
          )}
        </>
      )}
    </SectionCard>
  );
}

function RewardBadge({ status }: { status: 'found' | 'missing' | 'unknown' }) {
  const t = useT();
  if (status === 'found') {
    return (
      <span className="inline-flex items-center gap-1.5 text-xs font-semibold text-success">
        <CheckCircle2 className="h-3.5 w-3.5" />
        {t("Belohnung gefunden")}
      </span>
    );
  }
  return (
    <span className={`inline-flex items-center gap-1.5 text-xs font-semibold ${status === 'missing' ? 'text-warning' : 'text-text-secondary'}`}>
      <CircleAlert className="h-3.5 w-3.5" />
      {status === 'missing' ? t("Belohnung fehlt") : t("Status unbekannt")}
    </span>
  );
}

function RewardGuide({ status }: { status: 'found' | 'missing' | 'unknown' }) {
  const t = useT();
  return (
    <div className="pt-4">
      <div className="mb-2 flex flex-wrap items-center justify-between gap-3">
        <p className="flex items-center gap-2 text-sm font-bold text-white">
          <Gift className="h-4 w-4 shrink-0 text-primary" />
          {t("Belohnung zum Einlösen")}
        </p>
        <RewardBadge status={status} />
      </div>
      <p className="mb-2 text-sm text-text-secondary">
        {t("Damit deine Lurker etwas zum Antippen haben, legst du in Twitch eine Kanalpunkte-Belohnung mit genau diesem Namen an. Den Preis bestimmst du selbst, zum Beispiel 10 Punkte.")}
      </p>
      <p className="mb-3 text-sm text-text-secondary">
        {t("Name der Belohnung")}: <span className="font-semibold text-white">Lurker Steuer</span>
      </p>
      <p className="mb-1.5 text-xs font-semibold text-text-secondary">
        {t("So legst du sie in Twitch an:")}
      </p>
      <ol className="list-decimal pl-5 space-y-1 text-sm text-text-secondary">
        <li>{t("Öffne dein Twitch Creator-Dashboard.")}</li>
        <li>{t("Geh zu Zuschauerbelohnungen.")}</li>
        <li>{t("Öffne Punkte und Belohnungen.")}</li>
        <li>
          {t("Wähle Individuelle Belohnung hinzufügen und nenn sie genau")}
          {' '}
          <span className="font-semibold text-white">Lurker Steuer</span>.
        </li>
      </ol>
      {status === 'unknown' && (
        <p className="text-xs text-text-secondary mt-3">
          {t("Wir konnten gerade nicht prüfen, ob die Belohnung schon da ist. Sobald die Verbindung wieder klappt, siehst du hier den Status.")}
        </p>
      )}
    </div>
  );
}
