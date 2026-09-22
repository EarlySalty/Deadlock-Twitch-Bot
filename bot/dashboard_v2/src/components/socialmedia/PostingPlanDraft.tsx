import { useEffect, useRef, useState, type ReactNode } from 'react';
import { Check, Loader2 } from 'lucide-react';
import {
  fetchPostingPlan,
  saveCategoryAutoPost,
  savePlatformSchedule,
  savePostingPlanSettings,
} from '@/api/socialMedia';
import type { PostingPlan } from '@/types/socialMedia';
import { useT } from '@/context/LanguageContext';
import { fehlerText } from './labels';

function editablePlan(plan: PostingPlan): string {
  return JSON.stringify({
    approval_mode: plan.approval_mode,
    timezone: plan.timezone,
    subtitles_enabled: plan.subtitles_enabled,
    platforms: plan.platforms.map(
      ({ platform, auto_post, posts_per_week, max_posts_per_day, post_times }) => ({
        platform,
        auto_post,
        posts_per_week,
        max_posts_per_day,
        post_times,
      }),
    ),
    categories: plan.categories.map(({ category_key, auto_post }) => ({ category_key, auto_post })),
  });
}

/** Alle schreibbaren Felder flach mit stabilem Schlüssel; Listen werden als
 * JSON verglichen. Basis für die feldweise Konfliktprüfung. */
function planeFelder(plan: PostingPlan): Record<string, string | number | boolean> {
  const felder: Record<string, string | number | boolean> = {
    approval_mode: plan.approval_mode,
    timezone: plan.timezone,
    subtitles_enabled: plan.subtitles_enabled,
  };
  for (const ziel of plan.platforms) {
    felder[`platform.${ziel.platform}.auto_post`] = ziel.auto_post;
    felder[`platform.${ziel.platform}.posts_per_week`] = ziel.posts_per_week;
    felder[`platform.${ziel.platform}.max_posts_per_day`] = ziel.max_posts_per_day;
    felder[`platform.${ziel.platform}.post_times`] = JSON.stringify(ziel.post_times);
  }
  for (const kategorie of plan.categories) {
    felder[`category.${kategorie.category_key}.auto_post`] = kategorie.auto_post;
  }
  return felder;
}

/** Der bestehende Vertrag besitzt mehrere Schreibendpunkte. Fortschritte werden
 * einzeln bestätigt; bei Teilfehlern bleiben Entwurf und Serverstand getrennt. */
export function PostingPlanDraft({
  streamer,
  plan,
  loading,
  loadError,
  onSaved,
  children,
}: {
  streamer: string;
  plan: PostingPlan | null;
  loading: boolean;
  loadError: unknown;
  onSaved: (plan: PostingPlan) => void;
  children: (state: {
    draft: PostingPlan | null;
    patch: (change: (plan: PostingPlan) => PostingPlan) => void;
    busy: boolean;
  }) => ReactNode;
}) {
  const t = useT();
  const [draft, setDraft] = useState<PostingPlan | null>(null);
  const [baseline, setBaseline] = useState<PostingPlan | null>(null);
  const [busy, setBusy] = useState(false);
  const [inputDirty, setInputDirty] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const [needsRefresh, setNeedsRefresh] = useState(false);
  const [version, setVersion] = useState(0);
  const saveLock = useRef(false);
  const effective = draft ?? plan;
  const dirty =
    inputDirty || Boolean(draft && baseline && editablePlan(draft) !== editablePlan(baseline));
  // Konflikt nur, wenn der Fremde ein Feld verändert hat, das auch der
  // Entwurf ändert und dabei weder Basis- noch Entwurfswert steht. Fremde
  // Änderungen an unberührten Feldern blockieren den Retry nicht.
  const basisFelder = draft && baseline ? planeFelder(baseline) : null;
  const entwurfFelder = draft ? planeFelder(draft) : null;
  const serverFelder = plan ? planeFelder(plan) : null;
  const conflict = Boolean(
    !busy &&
      basisFelder &&
      entwurfFelder &&
      serverFelder &&
      Object.keys(entwurfFelder).some(
        (feld) =>
          entwurfFelder[feld] !== basisFelder[feld] &&
          serverFelder[feld] !== basisFelder[feld] &&
          serverFelder[feld] !== entwurfFelder[feld],
      ),
  );
  useEffect(() => {
    const warn = (event: BeforeUnloadEvent) => {
      if (dirty || saveLock.current) {
        event.preventDefault();
        event.returnValue = '';
      }
    };
    window.addEventListener('beforeunload', warn);
    return () => window.removeEventListener('beforeunload', warn);
  }, [dirty]);
  const patch = (change: (current: PostingPlan) => PostingPlan) => {
    if (!effective || busy || loadError) return;
    setBaseline((current) => current ?? plan);
    setDraft((current) => change(current ?? effective));
    setMessage(null);
  };
  const reset = () => {
    setDraft(null);
    setBaseline(null);
    setInputDirty(false);
    setMessage(null);
    setFailed(false);
    setNeedsRefresh(false);
    setVersion((v) => v + 1);
  };
  const save = async () => {
    if (!effective || !plan || loadError || conflict || needsRefresh || saveLock.current) return;
    saveLock.current = true;
    setBusy(true);
    setMessage(null);
    setFailed(false);
    // Feste Ausgangsbasis dieses Entwurfszyklus: Nur Felder, die der Entwurf
    // gegenüber dieser Basis ändert, werden geschrieben. Die Basis übersteht
    // den Neustart im Fehlerpfad, damit ein Retry fremde Änderungen an
    // unberührten Feldern nicht mit dem alten Entwurf überschreibt.
    const base = draft && baseline ? baseline : plan;
    let completed = 0;
    const accept = (next: PostingPlan) => {
      completed += 1;
      onSaved(next);
    };
    // Moduswechsel, der die Automatik einschränkt, wird zuerst wirksam;
    // Vollautomatik bleibt dagegen der letzte Schritt nach allen Zielen.
    const beschraenkung = (mode: PostingPlan['approval_mode']) =>
      mode === 'manual' ? 2 : mode === 'veto_window' ? 1 : 0;
    const modusZuerst =
      beschraenkung(effective.approval_mode) > beschraenkung(base.approval_mode);
    const schreibeEinstellungen = async () => {
      const settings: Parameters<typeof savePostingPlanSettings>[1] = {};
      if (effective.approval_mode !== base.approval_mode)
        settings.approval_mode = effective.approval_mode;
      if (effective.timezone !== base.timezone) settings.timezone = effective.timezone;
      if (effective.subtitles_enabled !== base.subtitles_enabled)
        settings.subtitles_enabled = effective.subtitles_enabled;
      if (Object.keys(settings).length) accept(await savePostingPlanSettings(streamer, settings));
    };
    const schreibeZiele = async () => {
      // Erst die einzelnen Ziele. Jeder Aufruf enthält die betroffene
      // Einstellung statt eines fremden Kanalzustands.
      for (const target of effective.platforms) {
        const current = base.platforms.find((p) => p.platform === target.platform);
        if (!current) continue;
        const payload: Partial<typeof target> = {};
        if (target.auto_post !== current.auto_post) payload.auto_post = target.auto_post;
        if (target.posts_per_week !== current.posts_per_week)
          payload.posts_per_week = target.posts_per_week;
        if (target.max_posts_per_day !== current.max_posts_per_day)
          payload.max_posts_per_day = target.max_posts_per_day;
        if (JSON.stringify(target.post_times) !== JSON.stringify(current.post_times))
          payload.post_times = target.post_times;
        if (Object.keys(payload).length)
          accept(await savePlatformSchedule(streamer, target.platform, payload));
      }
      for (const target of effective.categories) {
        if (
          base.categories.find((c) => c.category_key === target.category_key)?.auto_post !==
          target.auto_post
        ) {
          accept(await saveCategoryAutoPost(streamer, target.category_key, target.auto_post));
        }
      }
    };
    try {
      if (modusZuerst) await schreibeEinstellungen();
      await schreibeZiele();
      if (!modusZuerst) await schreibeEinstellungen();
      setDraft(null);
      setBaseline(null);
      setInputDirty(false);
      setVersion((v) => v + 1);
      setMessage(t('Änderungen gespeichert.'));
    } catch (error) {
      // Nach einem Netzabbruch kann die letzte Änderung trotzdem gespeichert
      // sein. Neu lesen, aber den gewünschten Entwurf nicht überschreiben.
      // Die Basis des Zyklus bleibt dabei stehen: Der Retry difft weiter
      // gegen sie, fremde Änderungen an unberührten Feldern fließen nicht
      // in den erneuten Schreibaufruf. Berührt der Fremde ein Entwurfsfeld,
      // greift stattdessen der Konflikt-Pfad.
      try {
        onSaved(await fetchPostingPlan(streamer));
      } catch {
        setNeedsRefresh(true);
      }
      setDraft(effective);
      setFailed(true);
      setMessage(
        `${completed ? t('Teilweise gespeichert. Dein Entwurf bleibt erhalten.') : t('Speichern fehlgeschlagen. Dein Entwurf bleibt erhalten.')} ${fehlerText(error, t) ?? ''}`,
      );
    } finally {
      saveLock.current = false;
      setBusy(false);
    }
  };
  const reload = async () => {
    if (saveLock.current) return;
    saveLock.current = true;
    setBusy(true);
    try {
      const current = await fetchPostingPlan(streamer);
      onSaved(current);
      setNeedsRefresh(false);
      setMessage(null);
    } catch {
      setMessage(t('Der aktuelle Stand ist nicht erreichbar. Dein Entwurf bleibt erhalten.'));
    } finally {
      saveLock.current = false;
      setBusy(false);
    }
  };
  return (
    <form
      className="space-y-5"
      data-unsaved={dirty || busy ? 'true' : undefined}
      onKeyDown={(event) => {
        if (event.key !== 'Enter' || !(event.target instanceof HTMLInputElement)) return;
        event.preventDefault();
        const form = event.currentTarget;
        event.target.blur();
        window.setTimeout(() => {
          if (form.isConnected) form.requestSubmit();
        }, 0);
      }}
      onInput={() => {
        setBaseline((current) => current ?? plan);
        setInputDirty(true);
        setMessage(null);
      }}
      onSubmit={(event) => {
        event.preventDefault();
        void save();
      }}
    >
      <div key={version}>{children({ draft: effective, patch, busy })}</div>
      <footer className="studio-savebar flex flex-wrap items-center justify-between gap-3 rounded-xl border border-border px-4 py-3">
        <p
          role={failed || conflict ? 'alert' : 'status'}
          className={`text-sm ${failed || conflict ? 'text-danger' : 'text-text-secondary'}`}
        >
          {conflict
            ? t(
                'Der Zeitplan wurde zwischenzeitlich geändert. Lade den aktuellen Stand mit Verwerfen.',
              )
            : (message ??
              (dirty ? t('Ungespeicherte Änderungen') : t('Keine ungespeicherten Änderungen')))}
        </p>
        <div className="flex flex-wrap gap-2">
          {needsRefresh && (
            <button
              type="button"
              className="studio-button"
              disabled={busy}
              onClick={() => void reload()}
            >
              {t('Serverstand erneut laden')}
            </button>
          )}
          <button
            type="button"
            className="studio-button"
            disabled={!dirty || busy || needsRefresh}
            onClick={reset}
          >
            {t('Verwerfen')}
          </button>
          <button
            type="submit"
            className="studio-primary"
            disabled={
              !dirty ||
              busy ||
              needsRefresh ||
              loading ||
              Boolean(loadError) ||
              conflict ||
              !effective
            }
          >
            {busy ? <Loader2 className="h-4 w-4 animate-spin" /> : <Check className="h-4 w-4" />}
            {t('Änderungen speichern')}
          </button>
        </div>
      </footer>
    </form>
  );
}
