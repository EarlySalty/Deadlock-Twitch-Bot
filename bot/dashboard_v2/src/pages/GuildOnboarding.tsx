import { useCallback, useEffect, useRef, useState } from 'react';
import { useT } from '../context/LanguageContext';

import { GUILD_ONBOARDING_ROOT, automaticGuild, type GuildOnboardingSession } from '../api/guildOnboarding';

export function GuildOnboardingPage() {
  const t = useT();
  const [session, setSession] = useState<GuildOnboardingSession | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(false);
  const [pending, setPending] = useState(false);
  const [assigned, setAssigned] = useState<boolean | null>(null);
  const attempted = useRef<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(false);
    try {
      const response = await fetch(`${GUILD_ONBOARDING_ROOT}/session`, { credentials: 'same-origin', cache: 'no-store' });
      if (response.status === 401) { setSession(null); return; }
      if (!response.ok) throw new Error('onboarding_unavailable');
      setSession(await response.json() as GuildOnboardingSession);
    } catch { setError(true); }
    finally { setLoading(false); }
  }, []);

  const choose = useCallback(async (id: string) => {
    setPending(true);
    setError(false);
    try {
      const response = await fetch(`${GUILD_ONBOARDING_ROOT}/select`, {
        method: 'POST', credentials: 'same-origin', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ guild_id: id }),
      });
      if (!response.ok) throw new Error('guild_not_available');
      const result = await response.json() as { active: boolean };
      setAssigned(result.active);
      await refresh();
    } catch { setError(true); }
    finally { setPending(false); }
  }, [refresh]);

  useEffect(() => { void refresh(); }, [refresh]);
  useEffect(() => {
    const id = session && automaticGuild(session);
    if (id && attempted.current !== id) {
      attempted.current = id;
      void choose(id);
    }
  }, [session, choose]);

  const active = assigned === true || session?.memberships.some(membership => membership.active);
  return <main className="min-h-screen bg-bg px-4 py-12 text-white">
    <section className="panel-card mx-auto max-w-2xl rounded-2xl p-6 md:p-8">
      <p className="text-xs font-bold uppercase tracking-wider text-primary">{t('Dein Einstieg')}</p>
      <h1 className="mt-2 text-2xl font-bold">{t('Wähle deine Community')}</h1>
      <p className="mt-3 text-sm leading-relaxed text-text-secondary">{t('Verbinde Twitch und Discord. Danach kannst du einen Server wählen, auf dem du Mitglied bist und der den Bot eingerichtet hat.')}</p>
      {loading && <p className="mt-5" role="status">{t('Einrichtung wird geladen …')}</p>}
      {error && <div role="alert" className="mt-5 rounded-lg border border-warning/40 p-3 text-warning">
        <p>{t('Die Einrichtung konnte gerade nicht abgeschlossen werden. Verbinde Discord erneut oder versuche es noch einmal.')}</p>
        <button type="button" onClick={() => void refresh()} className="mt-3 min-h-11 underline">{t('Erneut versuchen')}</button>
      </div>}
      {!loading && !session && <a className="mt-6 inline-flex min-h-11 items-center rounded-lg bg-primary px-4 py-2 font-bold text-bg" href={`${GUILD_ONBOARDING_ROOT}/twitch/login`}>{t('Mit Twitch anmelden')}</a>}
      {session && <>
        <p className="mt-5 text-sm text-text-secondary">{t('Mit Twitch angemeldet als {name}', { name: session.twitch_login })}</p>
        <a className="mt-4 inline-flex min-h-11 items-center rounded-lg border border-primary px-4 py-2 font-semibold text-primary" href={`${GUILD_ONBOARDING_ROOT}/discord/login`}>{t(session.discord_linked ? 'Discord erneut verbinden' : 'Mit Discord verbinden')}</a>
        {session.discord_linked && !session.guilds.length && <p role="status" className="mt-5 text-sm text-text-secondary">{t('Auf deinen Discord-Servern ist noch kein eingerichteter Bot verfügbar. Bitte den Server-Owner, den Bot hinzuzufügen und /setup auszuführen.')}</p>}
        {session.discord_linked && session.guilds.length > 0 && <div className="mt-5 space-y-3" aria-label={t('Verfügbare Server')}>
          {session.guilds.map(guild => {
            const membership = session.memberships.find(item => item.guild_id === guild.guild_id);
            return <div key={guild.guild_id} className="rounded-xl border border-border bg-black/30 p-4">
              <h2 className="font-semibold">{guild.name || guild.guild_id}</h2>
              <p className="mt-1 text-sm text-text-secondary">{t(membership?.active ? 'Du bist Mitglied dieser Community.' : membership ? 'Deine Aufnahme wartet auf Freigabe durch den Server-Admin.' : guild.auto_partner_default ? 'Du wirst direkt Partner dieses Servers.' : 'Ein Server-Admin muss deine Aufnahme freigeben.')}</p>
              {!membership && <button type="button" disabled={pending} onClick={() => void choose(guild.guild_id)} className="mt-3 min-h-11 rounded-lg bg-primary px-4 py-2 font-bold text-bg disabled:opacity-50">{t('Diesen Server wählen')}</button>}
            </div>;
          })}
        </div>}
        {pending && <p className="mt-4 text-sm" role="status">{t('Deine Serverwahl wird gespeichert …')}</p>}
        {active && <a className="mt-6 inline-flex min-h-11 items-center rounded-lg bg-primary px-4 py-2 font-bold text-bg" href="/twitch/auth/login?next=%2Ftwitch%2Fdashboard">{t('Weiter zum Dashboard')}</a>}
        {assigned === false && <p className="mt-5 text-sm" role="status">{t('Deine Aufnahme wartet auf Freigabe durch den Server-Admin.')}</p>}
      </>}
    </section>
  </main>;
}
