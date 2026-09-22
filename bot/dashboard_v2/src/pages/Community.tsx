import { useEffect, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { ArrowUpRight, CalendarDays, CheckCircle2, Clock3, Gamepad2, Info, Radio, RefreshCw, ShieldCheck, Users } from 'lucide-react';
import { fetchCommunity } from '../api/community';
import { usePlan } from '../context/PlanContext';
import { useT } from '../context/LanguageContext';
import { isPreviewModeEnabled } from '../preview/routes';
import type { CommunityData, CommunityLobby, CommunityProfile, CommunityRecommendation, CommunitySchedule } from '../types/community';
import { canOpenLobby, COMMUNITY_INVITE, directoryFresh, directoryMessage, lobbyFit, modeLabel, modeSource, rankAverageLabel, rankLabel, STREAMER_VC, twitchLink, voiceLink, WEEKDAYS, windowLabel } from '../utils/community';

const actionClass = 'inline-flex items-center justify-center gap-2 rounded-xl border border-primary/30 bg-primary/10 px-4 py-2.5 text-sm font-semibold text-primary hover:bg-primary/20 focus-visible:outline-2 focus-visible:outline-primary';
const mutedActionClass = 'inline-flex items-center justify-center gap-2 rounded-xl border border-white/10 px-3 py-2 text-sm text-text-secondary hover:text-white focus-visible:outline-2 focus-visible:outline-primary';

function ProfileChips({ profile }: { profile: CommunityProfile }) {
  return <div className="flex flex-wrap items-center gap-2 text-xs">
    <span className="rounded-lg border border-white/10 bg-white/5 px-2.5 py-1.5" title={profile.rank_updated_at ? `Steam-Rang vom ${new Date(profile.rank_updated_at * 1000).toLocaleDateString('de-DE')}` : 'Fehlende, unbestätigte oder veraltete Ränge werden nicht geschätzt.'}>{rankLabel(profile)}</span>
    <span className="rounded-lg border border-white/10 bg-white/5 px-2.5 py-1.5" title={modeSource(profile)}>{modeLabel(profile.mode)}</span>
    {profile.mode && <span className="text-text-secondary">{profile.mode_source === 'steam_history' ? 'aus Spielhistorie' : 'Titelhinweis · unbestätigt'}</span>}
  </div>;
}

export function StreamerMeeting() {
  const t = useT();
  return <aside className="panel-card rounded-2xl border border-primary/25 p-5 sm:p-6" aria-label="Streamer-Treffpunkt">
    <div className="flex flex-wrap items-start justify-between gap-4">
      <div className="flex gap-3">
        <div className="rounded-xl bg-primary/15 p-3 text-primary"><Radio className="h-6 w-6" aria-hidden="true" /></div>
        <div><p className="text-xs font-semibold uppercase tracking-widest text-primary">{t('Streamer zu Streamer')}</p>
          <h2 className="mt-1 text-xl font-semibold text-white">{t('Nicht nur gleichzeitig live. Gemeinsam streamen.')}</h2>
          <p className="mt-2 max-w-2xl text-sm text-text-secondary">{t('Verabredet euch im Streamer-VC: Duo, gemeinsamer Abend oder erst einmal kennenlernen. Vorher kurz klären, wer mit auf Sendung sein möchte.')}</p>
        </div>
      </div>
      <a href={voiceLink(STREAMER_VC)!} target="_blank" rel="noopener noreferrer" className={actionClass}>{t('Zum Streamer-VC')}<ArrowUpRight className="h-4 w-4" aria-hidden="true" /></a>
    </div>
    <div className="mt-5 flex items-start gap-2 border-t border-white/10 pt-4 text-xs leading-relaxed text-text-secondary">
      <ShieldCheck className="mt-0.5 h-4 w-4 shrink-0 text-primary" aria-hidden="true" />
      <p>{t('Die Streamer-Rolle hat in den vorgesehenen Sprachkanälen erweiterte Rechte: Personen verschieben und den Kanalzugriff sperren. Das ist kein serverweiter Bann. Bei Problemen bitte beim Mod-Team oder der Serverleitung melden.')}</p>
    </div>
  </aside>;
}

export function ScheduleMap({ own, other, ownLogin, otherLogin }: { own: CommunitySchedule; other?: CommunitySchedule; ownLogin: string; otherLogin?: string }) {
  const value = (slots: number[] | undefined, index: number) => Math.min(1, Math.max(0, slots?.[index] ?? 0));
  return <div>
    <div className="overflow-x-auto pb-2" tabIndex={0} role="region" aria-label="Historische Streamzeiten, Montag bis Sonntag, Zeit in Berlin">
      <div className="min-w-[570px] space-y-1.5">
        <div className="ml-9 flex justify-between pr-1 text-[10px] text-text-secondary" aria-hidden="true"><span>00:00</span><span>06:00</span><span>12:00</span><span>18:00</span><span>24:00</span></div>
        {WEEKDAYS.map((day, weekday) => <div key={day} className="grid items-center gap-0.5" style={{ gridTemplateColumns: '28px repeat(48, minmax(8px, 1fr))' }}>
          <span className="text-xs text-text-secondary">{day}</span>
          {Array.from({ length: 48 }, (_, halfHour) => {
            const index = weekday * 48 + halfHour;
            const a = value(own.slots, index); const b = value(other?.slots, index);
            const both = a >= 0.15 && b >= 0.15;
            const label = `${day} ${Math.floor(halfHour / 2).toString().padStart(2, '0')}:${halfHour % 2 ? '30' : '00'} · ${ownLogin}: ${Math.round(a * 100)} %${otherLogin ? ` · ${otherLogin}: ${Math.round(b * 100)} %` : ''} gewichtete Häufigkeit`;
            return <span key={halfHour} role="img" aria-label={label} title={label}
              className={`h-6 rounded-sm ${both ? 'bg-primary ring-1 ring-inset ring-primary/60' : a >= 0.15 ? 'bg-primary/30' : b >= 0.15 ? 'bg-accent/40' : 'bg-white/5'}`} />;
          })}
        </div>)}
      </div>
    </div>
    <div className="mt-3 flex flex-wrap gap-x-5 gap-y-2 text-xs text-text-secondary">
      <span className="inline-flex items-center gap-2"><i className="h-2.5 w-2.5 rounded-sm bg-primary" />Gemeinsame Zeitfenster</span>
      <span className="inline-flex items-center gap-2"><i className="h-2.5 w-2.5 rounded-sm bg-primary/30" />{ownLogin}</span>
      {otherLogin && <span className="inline-flex items-center gap-2"><i className="h-2.5 w-2.5 rounded-sm bg-accent/40" />{otherLogin}</span>}
    </div>
    <p className="mt-3 text-xs leading-relaxed text-text-secondary">Europe/Berlin · je Kästchen 30 Minuten · neuere Streams zählen stärker. Das sind beobachtete Gewohnheiten, keine zugesagten Termine.</p>
  </div>;
}

export function RecommendationCard({ person, selected, onSelect }: { person: CommunityRecommendation; selected: boolean; onSelect: () => void }) {
  const href = twitchLink(person.login);
  return <article className={`rounded-2xl border p-4 sm:p-5 ${selected ? 'border-primary/50 bg-primary/5' : 'border-white/10 bg-white/[0.02]'}`}>
    <div className="flex items-start justify-between gap-3">
      <div className="min-w-0">
        <div className="flex flex-wrap items-center gap-2">
          <h3 className="break-all font-semibold text-white">{person.login}</h3>
          {person.live_state === 'live' && <span className="inline-flex items-center gap-1 rounded-full bg-accent/15 px-2 py-0.5 text-[10px] font-bold uppercase tracking-wide text-accent"><Radio className="h-3 w-3" />Live</span>}
          {person.live_state === 'unknown' && <span className="text-[10px] text-text-secondary">Livestatus unbekannt</span>}
        </div>
        <p className="mt-1 text-xs text-text-secondary">{person.schedule.sessions} ausgewertete Streams{person.current_game ? ` · jetzt ${person.current_game}` : ''}</p>
      </div>
      <div className="shrink-0 text-right" title="Signalpunkte, keine Erfolgswahrscheinlichkeit. Fehlende Informationen geben keine Bonuspunkte.">
        <span className={`text-2xl font-semibold tabular-nums ${person.compatible ? 'text-primary' : 'text-text-secondary'}`}>{person.score ?? '—'}</span>
        <div className="text-[10px] text-text-secondary">{person.score === null ? 'noch keine Wertung' : 'Signalpunkte / 100'}</div>
      </div>
    </div>
    <div className="mt-4"><ProfileChips profile={person.profile} /></div>
    {person.shared_windows.length > 0 && <div className="mt-4 flex flex-wrap gap-2">{person.shared_windows.slice(0, 3).map(window => <span key={`${window.weekday}-${window.start_minute}`} className="inline-flex items-center gap-1.5 rounded-lg bg-white/5 px-2.5 py-1.5 text-xs"><Clock3 className="h-3 w-3 text-primary" />{windowLabel(window)}</span>)}</div>}
    <div className="mt-4 space-y-1.5 text-xs">
      {person.reasons.map(reason => <p key={reason} className="flex items-start gap-2 text-text-secondary"><CheckCircle2 className="mt-0.5 h-3.5 w-3.5 shrink-0 text-primary" />{reason}</p>)}
      {person.warnings.map(warning => <p key={warning} className="flex items-start gap-2 text-text-secondary"><Info className="mt-0.5 h-3.5 w-3.5 shrink-0" />{warning}</p>)}
    </div>
    {person.both_live_same_game && <p className="mt-3 rounded-lg border border-accent/20 bg-accent/5 p-2.5 text-xs text-accent">Ihr seid gerade im selben Spiel live. Erst anfragen, ob Mitspielen oder ein gemeinsamer Stream passt.</p>}
    <div className="mt-4 flex flex-wrap items-center justify-between gap-2 border-t border-white/10 pt-3">
      <button type="button" aria-pressed={selected} className={mutedActionClass} onClick={onSelect}><CalendarDays className="h-4 w-4" />Zeiten vergleichen</button>
      {href && <a href={href} target="_blank" rel="noopener noreferrer" className="inline-flex items-center gap-1 text-sm text-primary">Stream öffnen<ArrowUpRight className="h-4 w-4" /></a>}
    </div>
  </article>;
}

export function LobbyCard({ lobby, fresh, profile }: { lobby: CommunityLobby; fresh: boolean; profile: CommunityProfile }) {
  const allowed = canOpenLobby(lobby, fresh);
  const fit = lobbyFit(lobby, profile);
  const capacity = lobby.user_limit ?? Math.max(6, lobby.member_count);
  const ratio = Math.min(100, lobby.member_count / capacity * 100);
  return <article className="rounded-2xl border border-white/10 bg-white/[0.02] p-4">
    <div className="flex items-start justify-between gap-2"><h3 className="break-words font-semibold text-white">{lobby.name}</h3>
      <span className={`shrink-0 rounded-full px-2 py-0.5 text-[10px] ${fresh ? 'bg-primary/10 text-primary' : 'bg-white/5 text-text-secondary'}`}>{fresh ? 'Voice aktiv' : 'Stand veraltet'}</span>
    </div>
    <div className="mt-3 flex items-center justify-between text-xs text-text-secondary"><span className="inline-flex items-center gap-1"><Users className="h-3.5 w-3.5" />{lobby.member_count}{lobby.user_limit ? ` / ${lobby.user_limit}` : ''} im Sprachkanal</span>
      <span>{lobby.slots_free === null ? 'Kein VC-Limit' : lobby.slots_free === 0 ? 'VC voll' : `${lobby.slots_free} VC-Plätze frei`}</span>
    </div>
    <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-white/5" role="meter" aria-label="Belegung des Sprachkanals" aria-valuenow={lobby.member_count} aria-valuemin={0} aria-valuemax={Math.max(capacity, lobby.member_count)}><div className="h-full rounded-full bg-primary/60" style={{ width: `${ratio}%` }} /></div>
    <div className="mt-3 flex flex-wrap gap-2 text-xs text-text-secondary"><span>{lobby.is_streamer_vc ? 'Streamer-Treffpunkt' : `Kategorie: ${modeLabel(lobby.mode)}`}</span><span>·</span><span>{rankAverageLabel(lobby.rank_average)}</span></div>
    {lobby.rank_samples > 0 && <p className="mt-1 text-[10px] text-text-secondary">Grobe Einschätzung aus {lobby.rank_samples} Discord-Rollen, nicht aus aktuellen Match-Rängen.</p>}
    <p className={`mt-3 text-xs ${fit.conflict ? 'text-warning' : 'text-text-secondary'}`}>{fit.explanation}</p>
    <div className="mt-4">
      {lobby.requester_present ? <span className="text-xs text-primary">Du bist bereits in diesem Sprachkanal.</span> : allowed ?
        <a href={voiceLink(lobby.channel_id)!} target="_blank" rel="noopener noreferrer" className={`${actionClass} w-full`}>Zum Sprachkanal<ArrowUpRight className="h-4 w-4" /></a> :
        <button type="button" disabled className="w-full rounded-xl border border-white/10 px-3 py-2.5 text-sm text-text-secondary opacity-60">{!fresh ? 'Aktualisierung erforderlich' : 'Sprachkanal voll'}</button>}
    </div>
  </article>;
}

export function CommunityView({ data, now, refreshing = false, refreshFailed = false, onRefresh }: { data: CommunityData; now: number; refreshing?: boolean; refreshFailed?: boolean; onRefresh: () => void }) {
  const [selected, setSelected] = useState<string | null>(null);
  const [onlyLive, setOnlyLive] = useState(false);
  const [onlyCompatible, setOnlyCompatible] = useState(false);
  const [onlyFree, setOnlyFree] = useState(true);
  const [showAll, setShowAll] = useState(false);
  const candidate = data.recommendations.find(person => person.login === selected) ?? data.recommendations[0];
  const visible = data.recommendations.filter(person => (!onlyLive || person.live_state === 'live') && (!onlyCompatible || person.compatible));
  const fresh = !refreshFailed && directoryFresh(data.discord, now);
  const message = directoryMessage(data.discord.status, fresh);
  const lobbies = data.discord.lobbies.filter(lobby => !onlyFree || (lobby.joinable && lobby.slots_free !== 0))
    .slice().sort((a, b) => lobbyFit(b, data.own_profile).points - lobbyFit(a, data.own_profile).points || b.member_count - a.member_count);
  return <div className="space-y-6" data-community-view>
    <StreamerMeeting />
    {refreshFailed && <p role="alert" className="rounded-xl border border-warning/20 bg-warning/5 p-3 text-sm text-warning">Aktualisierung fehlgeschlagen. Historische Vorschläge bleiben sichtbar; die Lobby-Einstiege sind vorsichtshalber deaktiviert.</p>}
    <section className="panel-card rounded-2xl p-5 sm:p-6" aria-labelledby="community-times">
      <div className="mb-5 flex flex-wrap items-start justify-between gap-4"><div><h2 id="community-times" className="flex items-center gap-2 text-lg font-semibold text-white"><CalendarDays className="h-5 w-5 text-primary" />Wann überschneiden sich eure Streams?</h2>
        <p className="mt-1 text-sm text-text-secondary">{data.days} Tage · {data.own_schedule.sessions} abgeschlossene Streams von {data.streamer}</p></div>
        {candidate && <label className="flex flex-col gap-1 text-xs text-text-secondary">Vergleichen mit<select aria-label="Vergleichen mit" value={candidate.login} onChange={event => setSelected(event.target.value)} className="max-w-full rounded-xl border border-white/10 bg-bg px-3 py-2 text-sm text-white">{data.recommendations.map(person => <option key={person.login} value={person.login}>{person.login}</option>)}</select></label>}
      </div>
      <ScheduleMap own={data.own_schedule} other={candidate?.schedule} ownLogin={data.streamer} otherLogin={candidate?.login} />
      {candidate && <p className="mt-4 rounded-xl border border-white/10 p-3 text-xs text-text-secondary">Mit {candidate.login}: <strong className="text-white">{(candidate.observed_overlap_minutes / 60).toLocaleString('de-DE', { maximumFractionDigits: 1 })} Stunden tatsächlich gleichzeitig gestreamt</strong> im betrachteten Zeitraum. Das ist nicht gleichbedeutend mit gemeinsam gespielten Matches.</p>}
    </section>
    <div className="grid items-start gap-6 xl:grid-cols-[minmax(0,1.5fr)_minmax(310px,1fr)]">
      <section className="min-w-0 panel-card rounded-2xl p-5 sm:p-6" aria-labelledby="community-streamers">
        <div className="flex items-center gap-2"><Users className="h-5 w-5 text-primary" /><h2 id="community-streamers" className="text-lg font-semibold text-white">Wer passt zu dir?</h2></div>
        <p className="mt-1 text-sm text-text-secondary">Passende Zeiten zuerst. Bekannte Ränge und Modi verfeinern die Auswahl.</p>
        <div className="mt-4 flex flex-wrap gap-4 text-xs text-text-secondary"><label className="flex items-center gap-2"><input type="checkbox" checked={onlyLive} onChange={e => setOnlyLive(e.target.checked)} />Jetzt live</label><label className="flex items-center gap-2"><input type="checkbox" checked={onlyCompatible} onChange={e => setOnlyCompatible(e.target.checked)} />Ohne bekannte Rang-/Moduskonflikte</label></div>
        <div className="mt-5 space-y-4">{(showAll ? visible : visible.slice(0, 6)).map(person => <RecommendationCard key={person.login} person={person} selected={candidate?.login === person.login} onSelect={() => setSelected(person.login)} />)}</div>
        {visible.length === 0 && <div className="mt-5 rounded-xl border border-dashed border-white/15 p-6 text-sm text-text-secondary">{data.recommendations.length ? 'Für diese Filter gibt es gerade keine Vorschläge.' : 'Noch keine anderen aktiven Partner mit passenden Daten vorhanden. Der Streamer-VC bleibt euer Treffpunkt.'}</div>}
        {visible.length > 6 && <button type="button" className={`${mutedActionClass} mt-4 w-full`} onClick={() => setShowAll(!showAll)}>{showAll ? 'Weniger anzeigen' : `Alle ${visible.length} Vorschläge ansehen`}</button>}
        <p className="mt-4 text-[11px] leading-relaxed text-text-secondary">{data.evaluated_count} von {data.candidate_count} anderen aktiven Partnern näher betrachtet. Bei großen Netzwerken werden bis zu 16 Kandidaten nach Zeitüberschneidung vorausgewählt. Signalpunkte sind keine Wahrscheinlichkeit und keine Zusage.</p>
      </section>
      <section className="min-w-0 panel-card rounded-2xl p-5 sm:p-6" aria-labelledby="community-lobbies">
        <div className="flex items-center justify-between gap-3"><h2 id="community-lobbies" className="flex items-center gap-2 text-lg font-semibold text-white"><Gamepad2 className="h-5 w-5 text-primary" />Jetzt im Discord</h2>
          <button type="button" aria-label="Community-Daten aktualisieren" title="Aktualisieren" disabled={refreshing} onClick={onRefresh} className="rounded-lg p-2 text-text-secondary hover:text-white disabled:opacity-50"><RefreshCw className={`h-4 w-4 ${refreshing ? 'animate-spin' : ''}`} /></button></div>
        <p className="mt-1 text-sm text-text-secondary">Aktive Sprachkanäle, auf die dein Discord-Konto zugreifen darf.</p>
        <div className="mt-4 flex flex-wrap items-center justify-between gap-2 text-xs text-text-secondary"><label className="flex items-center gap-2"><input type="checkbox" checked={onlyFree} onChange={e => setOnlyFree(e.target.checked)} />Nur mit VC-Platz</label><span>{fresh ? 'Frisch geprüft' : 'Keine Live-Bestätigung'}</span></div>
        {message && <div role="status" className="mt-4 rounded-xl border border-warning/20 bg-warning/5 p-4 text-sm text-text-secondary"><p>{message}</p>{data.discord.status === 'link_required' && <a href="/twitch/verwaltung" className="mt-3 inline-block text-primary">Discord-Verbindung verwalten →</a>}{data.discord.status === 'membership_unconfirmed' && <a href={COMMUNITY_INVITE} target="_blank" rel="noopener noreferrer" className="mt-3 inline-block text-primary">Community-Discord öffnen →</a>}</div>}
        <div className="mt-4 space-y-3">{lobbies.map(lobby => <LobbyCard key={lobby.channel_id} lobby={lobby} fresh={fresh} profile={data.own_profile} />)}</div>
        {fresh && lobbies.length === 0 && <p className="mt-4 rounded-xl border border-dashed border-white/15 p-5 text-sm text-text-secondary">{data.discord.lobbies.length ? 'Die sichtbaren Sprachkanäle sind gerade voll. Deaktiviere den Filter, um sie zu sehen.' : 'Gerade keine belegte, für dich sichtbare Community-Lobby gefunden.'}</p>}
        <p className="mt-4 text-[11px] leading-relaxed text-text-secondary">Ein freier VC-Platz ist kein garantierter Ingame-Platz. Der Klick öffnet Discord; Beitritt und Rechte prüft Discord erneut. Keine automatische Verschiebung, kein Audio im Browser. Aktualisierung alle 30 Sekunden.</p>
      </section>
    </div>
    <details className="panel-card rounded-2xl p-5 text-sm text-text-secondary"><summary className="cursor-pointer font-medium text-white">Wie entstehen die Vorschläge?</summary><div className="mt-3 space-y-2 leading-relaxed"><p>Abgeschlossene Streams liefern halbstündige Wochenprofile in Berliner Zeit. Tatsächlich gleichzeitig gestreamte Minuten werden separat aus den Session-Zeiten berechnet. Für eine Wertung sind mindestens drei Sessions je Person nötig.</p><p>Bis zu 65 Punkte kommen aus den Zeiten, 15 aus gemeinsamen Spielen, 15 aus bestätigten, höchstens 14 Tage alten Steam-Rängen und 5 aus passenden Modi. Ein Titelhinweis gibt höchstens 2 Moduspunkte. Unbekannte Werte bleiben unbekannt.</p><p>Spielhistorie berücksichtigt explizite Modi aus den letzten 14 Tagen. Ein einzelnes Spiel genügt nicht: mindestens drei passende Spiele und ein überwiegender Modus sind nötig. Aktuelle Titel sind Absichtshinweise, keine verifizierten Match-Daten.</p><p>Ein größerer Rangunterschied oder unterschiedliche Modi werden gekennzeichnet. Zeiten, Spielweise und freie Plätze solltet ihr trotzdem kurz miteinander absprechen.</p></div></details>
  </div>;
}

export function Community({ streamer, days }: { streamer: string | null; days: number }) {
  const { isDemoMode } = usePlan();
  const demo = isDemoMode || isPreviewModeEnabled();
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const refreshClock = () => setNow(Date.now());
    const timer = window.setInterval(refreshClock, 5_000);
    window.addEventListener('focus', refreshClock);
    return () => { window.clearInterval(timer); window.removeEventListener('focus', refreshClock); };
  }, []);
  const query = useQuery({
    queryKey: ['community', streamer, Math.min(90, Math.max(7, days))],
    queryFn: () => fetchCommunity(streamer!, days),
    enabled: !!streamer && !demo,
    staleTime: 15_000,
    refetchInterval: 30_000,
    refetchOnWindowFocus: true,
  });
  return <div className="space-y-6">
    <header className="flex flex-wrap items-start justify-between gap-4"><div><p className="text-xs font-semibold uppercase tracking-widest text-primary">Community</p><h1 className="mt-1 text-2xl font-semibold text-white sm:text-3xl">Zusammen spielen &amp; streamen</h1><p className="mt-2 text-sm text-text-secondary">Finde Menschen für deinen nächsten gemeinsamen Abend – und sieh, wo gerade etwas los ist.</p></div>{query.data && !demo && <ProfileChips profile={query.data.own_profile} />}</header>
    {demo ? <><StreamerMeeting /><p className="panel-card rounded-2xl p-6 text-sm text-text-secondary">Die Demo liest keine persönlichen Steam- oder Discord-Livedaten. Melde dich mit deinem Partner-Konto an, um echte Vorschläge und sichtbare Lobbys zu sehen.</p></> : !streamer ? <p className="panel-card rounded-2xl p-6 text-sm text-text-secondary">Bitte einen Streamer auswählen.</p> : query.isPending ? <><StreamerMeeting /><div role="status" className="panel-card flex items-center gap-3 rounded-2xl p-8 text-text-secondary"><RefreshCw className="h-5 w-5 animate-spin text-primary" />Streamzeiten und Community-Daten werden geladen …</div></> : query.data ? <CommunityView key={streamer} data={query.data} now={now} refreshing={query.isFetching} refreshFailed={query.isError} onRefresh={() => void query.refetch()} /> : <><StreamerMeeting /><div role="alert" className="panel-card rounded-2xl p-6"><p className="text-text-secondary">{query.error instanceof Error ? query.error.message : 'Community-Daten konnten nicht geladen werden.'}</p><button type="button" onClick={() => void query.refetch()} className={`${actionClass} mt-4`}>Erneut versuchen</button></div></>}
  </div>;
}
