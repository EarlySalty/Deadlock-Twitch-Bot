import { landingText, landingLink, landingLocale, isEnglishLanding } from "@/lib/landingLocale";
import { StrictMode, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { Award, CalendarDays, Check, Crown, ExternalLink, LogIn, LogOut, Play, Send, ShieldCheck, Trophy, Film } from 'lucide-react'
import './clips.css'

type Clip = {
  id: number; clip_id: string; clip_url: string; title: string; channel: string
  channel_name?: string | null; thumbnail_url?: string | null
  votes: number; my_vote: boolean; my_own: boolean; rank?: number
}
type Current = {
  month: string; month_label: string; phase: 'submission' | 'voting'; phase_ends_at: string
  submissions: Clip[]; top3: Clip[]; total: number; next_offset: number | null
}
type Session = {
  authenticated: boolean; provider: string | null; display_name: string | null
  discord_authenticated: boolean; is_admin: boolean; can_submit: boolean
  submissions_used: number; submissions_limit: number; can_vote: boolean
  votes_used: number; votes_limit: number; eligibility_unavailable: boolean
  discord_eligibility?: { account_age_ok: boolean; member_age_ok: boolean; present: boolean } | null
}
type ArchiveMonth = { month: string; month_label: string; winners: Clip[] }
type Archive = { months: ArchiveMonth[]; next_before: string | null }

async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, {
    credentials: 'same-origin', ...init,
    headers: init?.body ? { 'content-type': 'application/json', ...init.headers } : init?.headers,
  })
  const data = await response.json().catch(() => ({}))
  if (!response.ok) throw new Error(data.message || landingText("Die Anfrage ist fehlgeschlagen. Bitte erneut versuchen."))
  return data as T
}
const message = (error: unknown) => error instanceof Error ? error.message : landingText("Die Anfrage ist fehlgeschlagen.")
const voteLabel = (votes: number) => `${votes} ${votes === 1 ? landingText("Stimme") : landingText("Stimmen")}`

function monthLabel(month: string, fallback: string): string {
  if (!isEnglishLanding() || !/^\d{4}-\d{2}$/.test(month)) return fallback;
  return new Date(month + '-01T12:00:00Z').toLocaleDateString(landingLocale(), { month: 'long', year: 'numeric', timeZone: 'Europe/Berlin' });
}

function embedUrl(clipId: string) {
  const query = new URLSearchParams({ clip: clipId, parent: window.location.hostname, autoplay: 'false' })
  return `https://clips.twitch.tv/embed?${query}`
}

function ClipPlayer({ clip }: { clip: Clip }) {
  const [playing, setPlaying] = useState(false)
  const player = useRef<HTMLDivElement>(null)
  const url = `https://clips.twitch.tv/${encodeURIComponent(clip.clip_id)}`
  return <div className="clip-player" ref={player}>
    {playing ? <iframe src={embedUrl(clip.clip_id)} title={isEnglishLanding() ? `${clip.title} by ${clip.channel_name || clip.channel}` : `${clip.title} von ${clip.channel_name || clip.channel}`}
      loading="lazy" allow="fullscreen" allowFullScreen /> : <button className="clip-preview" type="button"
      onClick={() => {
        if ((player.current?.clientWidth || 0) < 400) window.open(url, '_blank', 'noopener,noreferrer')
        else setPlaying(true)
      }} aria-label={isEnglishLanding() ? `Play clip: ${clip.title}` : `Clip abspielen: ${clip.title}`}>
      {clip.thumbnail_url ? <img src={clip.thumbnail_url} alt="" loading="lazy" /> : <span className="preview-letter" aria-hidden="true">D</span>}
      <span className="play-label"><Play size={23} fill="currentColor" />{landingText(" Clip abspielen")}</span>
    </button>}
    <a className="player-fallback" href={landingLink(url)} target="_blank" rel="noopener noreferrer">{landingText("Auf Twitch ansehen ")}<ExternalLink size={13} /></a>
  </div>
}

function App() {
  const [current, setCurrent] = useState<Current | null>(null)
  const [session, setSession] = useState<Session | null>(null)
  const [archive, setArchive] = useState<Archive>({ months: [], next_before: null })
  const [selectedMonth, setSelectedMonth] = useState('')
  const [hidden, setHidden] = useState<Clip[]>([])
  const [url, setUrl] = useState('')
  const [notice, setNotice] = useState('')
  const [loadError, setLoadError] = useState('')
  const [archiveError, setArchiveError] = useState('')
  const [busy, setBusy] = useState(false)
  const generation = useRef(0)
  const loadedClips = useRef(24)
  const loadedMonths = useRef(24)

  const load = useCallback(async () => {
    const request = ++generation.current
    const [c, s] = await Promise.all([api<Current>('/clips/api/current'), api<Session>('/clips/api/session')])
    while (c.next_offset !== null && c.submissions.length < loadedClips.current) {
      const next = await api<Current>(`/clips/api/current?offset=${c.next_offset}`)
      if (next.month !== c.month) return
      c.submissions.push(...next.submissions)
      c.next_offset = next.next_offset
    }
    if (request !== generation.current) return
    setCurrent(c); setSession(s); setLoadError('')
    try {
      const a = await api<Archive>('/clips/api/archive')
      while (a.next_before && a.months.length < loadedMonths.current) {
        const next = await api<Archive>(`/clips/api/archive?before=${encodeURIComponent(a.next_before)}`)
        a.months.push(...next.months)
        a.next_before = next.next_before
      }
      if (request === generation.current) {
        setArchive(a); setArchiveError('')
      }
    } catch (error) { setArchiveError(message(error)) }
    if (s.is_admin) {
      const moderation = await api<{ submissions: Clip[] }>('/clips/api/admin/submissions')
      if (request === generation.current) setHidden(moderation.submissions)
    } else setHidden([])
  }, [])

  useEffect(() => {
    const login = new URLSearchParams(window.location.search).get('login')
    if (login === 'fehler') setNotice(landingText("Die Anmeldung hat nicht geklappt. Bitte erneut anmelden."))
    if (login === 'abgebrochen') setNotice(landingText("Die Anmeldung wurde abgebrochen."))
    void load().catch(error => setLoadError(message(error)))
    return () => { generation.current++ }
  }, [load])

  useEffect(() => {
    if (!current) return
    const timer = window.setInterval(() => {
      if (!busy && document.visibilityState === 'visible') void load().catch(error => setNotice(message(error)))
    }, 60_000)
    return () => window.clearInterval(timer)
  }, [busy, current?.month, load])

  const action = async (path: string, body: object, success: string) => {
    setBusy(true); setNotice('')
    try {
      await api(path, { method: 'POST', body: JSON.stringify(body) })
      setNotice(success)
      await load()
    } catch (error) { setNotice(message(error)) }
    finally { setBusy(false) }
  }
  const submit = async (event: React.FormEvent) => {
    event.preventDefault()
    await action('/clips/api/submit', { clip_url: url }, landingText("Clip eingereicht."))
  }
  const hide = async (id: number, hideClip: boolean) => {
    const reason = window.prompt(hideClip ? landingText("Warum soll der Clip ausgeblendet werden? Die Stimmen bleiben erhalten.") : landingText("Warum soll der Clip wieder angezeigt werden? Abgeschlossene Plätze bleiben unverändert."))?.trim()
    if (!reason) return
    if (reason.length > 500) { setNotice(landingText("Die Begründung darf höchstens 500 Zeichen haben.")); return }
    await action('/clips/api/admin/hide', { submission_id: id, hidden: hideClip, reason }, hideClip ? landingText("Clip ausgeblendet. Stimmen bleiben erhalten.") : landingText("Clip wieder sichtbar."))
  }
  const loadMore = async () => {
    if (!current || current.next_offset === null) return
    setBusy(true)
    try {
      const next = await api<Current>(`/clips/api/current?offset=${current.next_offset}`)
      loadedClips.current = current.submissions.length + next.submissions.length
      setCurrent(previous => previous?.month === next.month ? {
        ...next, submissions: [...new Map([...previous.submissions, ...next.submissions].map(clip => [clip.id, clip])).values()],
      } : next)
    } catch (error) { setNotice(message(error)) }
    finally { setBusy(false) }
  }
  const olderMonths = async () => {
    if (!archive.next_before) return
    setBusy(true)
    try {
      const next = await api<Archive>(`/clips/api/archive?before=${encodeURIComponent(archive.next_before)}`)
      loadedMonths.current = archive.months.length + next.months.length
      setArchive(previous => ({ months: [...previous.months, ...next.months], next_before: next.next_before }))
    } catch (error) { setArchiveError(message(error)) }
    finally { setBusy(false) }
  }
  const topIds = useMemo(() => new Set(current?.top3.map(clip => clip.id)), [current?.top3])
  const others = current?.submissions.filter(clip => !topIds.has(clip.id)) || []

  if (!current || !session) return <main className="clips-page"><section className="loading-card" aria-live="polite">
    <img src="/brand/logo/logo-192.png" width="56" height="56" alt={landingText("Deutsche Deadlock Community")} />
    <h1>{landingText("Clip des Monats")}</h1>
    <p>{loadError || landingText("Der Wettbewerb wird geladen …")}</p>
    {loadError && <button className="button button--gold" onClick={() => void load().catch(error => setLoadError(message(error)))}>{landingText("Erneut versuchen")}</button>}
    <a href={landingLink("/")}>{landingText("Zur Community")}</a>
  </section></main>

  const submissionsOpen = current.phase === 'submission'
  const votesLeft = Math.max(0, session.votes_limit - session.votes_used)
  const eligibility = session.discord_eligibility
  const votingReason = !session.discord_authenticated ? landingText("Zum Abstimmen mit Discord anmelden.")
    : session.eligibility_unavailable ? landingText("Die Mitgliedschaft kann gerade nicht geprüft werden. Bitte erneut versuchen.")
    : !eligibility?.account_age_ok ? landingText("Dein Discord-Konto muss mindestens 30 Tage alt sein.")
    : !eligibility?.present || !eligibility.member_age_ok ? landingText("Du musst seit mindestens 7 Tagen Mitglied der Community sein.")
    : !votesLeft ? landingText("Du hast deine fünf Stimmen für diesen Monat vergeben.") : `${votesLeft} ${votesLeft === 1 ? landingText("Stimme übrig") : landingText("Stimmen übrig")}`
  const shownMonth = archive.months.find(month => month.month === selectedMonth) || archive.months[0]
  const deadline = new Date(current.phase_ends_at).toLocaleDateString(landingLocale(), { day: 'numeric', month: 'long', timeZone: 'Europe/Berlin' })

  const actions = (clip: Clip) => <div className="clip-card__footer">
    <b>{voteLabel(clip.votes)}</b>
    {!submissionsOpen && <button className={clip.my_vote ? 'vote-button vote-button--done' : 'vote-button'}
      type="button" disabled={busy || !session.can_vote || clip.my_vote || clip.my_own}
      title={clip.my_own ? landingText("Für eigene Clips ist keine Stimme möglich.") : session.can_vote ? undefined : votingReason}
      onClick={() => void action('/clips/api/vote', { submission_id: clip.id }, landingText("Stimme gespeichert."))}>
      {clip.my_vote ? <><Check size={16} />{landingText(" Gewählt")}</> : clip.my_own ? landingText("Dein Clip") : landingText("Stimme geben")}
    </button>}
    {session.is_admin && <button className="moderate-button" type="button" disabled={busy} onClick={() => void hide(clip.id, true)}>{landingText("Ausblenden")}</button>}
  </div>

  return <main className="clips-page">
    <a className="skip-link" href={landingLink("#aktuell")}>{landingText("Zu den Clips")}</a>
    <header className="clip-header">
      <a className="brand-mark" href={landingLink("/")} aria-label={landingText("Deutsche Deadlock Community")}>
        <img src="/brand/logo/logo-192.png" width="40" height="40" alt="" />
        <span><b>{landingText("Deutsche Deadlock")}</b><small>Community</small></span>
      </a>
      <nav aria-label={landingText("Clip-Wettbewerb")}><a href={landingLink("#aktuell")}>Clips</a><a href={landingLink("#archiv")}>{landingText("Archiv")}</a>
        {session.authenticated && <button className="link-button" type="button" disabled={busy}
          onClick={() => void action('/clips/auth/logout', {}, landingText("Du bist abgemeldet."))}><LogOut size={16} /><span>{landingText("Abmelden")}</span></button>}
      </nav>
    </header>

    <section className="hero">
      <div className="hero-copy"><p className="eyebrow"><Trophy size={17} /> {monthLabel(current.month, current.month_label)}</p>
        <h1>{landingText("Dein Moment.")}<br /><span>{landingText("Unser Clip des Monats.")}</span></h1>
        <p className="hero-text">{landingText("Der letzte Treffer, der größte Fail, die Rettung in letzter Sekunde. Zeig uns, was in unserem Partnernetzwerk passiert.")}</p>
        <div className="phase-pill"><CalendarDays size={17} /><b>{submissionsOpen ? landingText("Clips einreichen") : landingText("Die Abstimmung läuft")}</b><span>{landingText("bis ")}{deadline}{landingText(", 00:00 Uhr")}</span></div>
      </div>
    </section>

    <aside className="rules-card" aria-label={landingText("Ablauf")}><div className="rules-title"><ShieldCheck size={19} />{landingText(" So läuft es")}</div>
      <ol><li aria-current={submissionsOpen ? 'step' : undefined}><b>{landingText("1. bis 21.")}</b><span>{landingText("Bis zu 3 Twitch-Clips pro Person einreichen.")}</span></li>
        <li aria-current={!submissionsOpen ? 'step' : undefined}><b>{landingText("22. bis Monatsende")}</b><span>{landingText("Mit Discord bis zu 5 Stimmen vergeben.")}</span></li>
        <li><b>{landingText("Am 1.")}</b><span>{landingText("Die Top 3 bleiben in der Hall of Fame.")}</span></li></ol>
    </aside>

    <section className="submission-panel" aria-label={submissionsOpen ? landingText("Clip einreichen") : landingText("Abstimmen")}>
      <div><p className="section-kicker">{submissionsOpen ? landingText("Dein Clip ist dran") : landingText("Du entscheidest")}</p>
        <h2>{submissionsOpen ? landingText("Was müssen wir gesehen haben?") : landingText("Gib deinen Favoriten eine Stimme.")}</h2>
        <p>{submissionsOpen ? landingText("Deadlock aus einem aktiven Partnerkanal. Höchstens 60 Tage alt. Ein Clip zählt pro Monat nur einmal.")
          : landingText("Dein Discord-Konto muss mindestens 30 Tage alt sein und du musst seit 7 Tagen auf unserem Server sein. Keine Stimmen für eigene Clips.")}</p>
      </div>
      {submissionsOpen && !session.authenticated ? <div className="login-actions">
        <a className="button button--gold" href={landingLink("/clips/auth/discord/login")}><LogIn size={18} />{landingText(" Mit Discord anmelden")}</a>
        <a className="button button--outline" href={landingLink("/twitch/auth/login?next=%2Fclips")}><LogIn size={18} />{landingText(" Mit Twitch anmelden")}</a>
      </div> : submissionsOpen ? <form className="submit-form" onSubmit={submit}>
        <div className="submit-label"><label htmlFor="clip-url">{landingText("Twitch-Clip-Adresse")}</label><span id="clip-usage" className="counter">{session.submissions_used}{landingText(" von ")}{session.submissions_limit}{landingText(" Clips diesen Monat · ")}{session.display_name}</span></div><div className="input-row">
          <input id="clip-url" type="url" required maxLength={500} placeholder="https://clips.twitch.tv/…" value={url} aria-describedby="clip-usage"
            onChange={event => setUrl(event.target.value)} disabled={busy || !session.can_submit} />
          <button className="button button--gold" type="submit" disabled={busy || !session.can_submit}><Send size={17} />{landingText(" Einreichen")}</button>
        </div>
      </form> : <div className="voting-access">
        {!session.discord_authenticated && <a className="button button--gold" href={landingLink("/clips/auth/discord/login")}><LogIn size={18} />{landingText(" Mit Discord abstimmen")}</a>}
        <p className="vote-status">{session.can_vote && <Check size={18} />}{votingReason}</p>
      </div>}
    </section>

    {notice && <div className="notice" role="status">{notice}</div>}
    <section className="top-section" id="aktuell">
      <div className="section-heading"><p className="section-kicker"><Crown size={17} />{landingText(" Aktuelle Top 3")}</p>
        <h2>{monthLabel(current.month, current.month_label)}</h2><p>{submissionsOpen ? landingText("Noch keine Stimmen. Die Abstimmung beginnt am 22.") : landingText("Vorläufiger Stand. Die Gewinner stehen am nächsten Monatsersten fest.")}</p>
      </div>
      <div className="top-grid">{current.top3.length ? current.top3.map((clip, index) => <article className={`top-card top-card--${index + 1}`} key={clip.id}>
        <div className="top-rank">{index === 0 ? <Crown size={20} /> : <Award size={19} />}{index + 1}{landingText(". Platz")}</div>
        <ClipPlayer clip={clip} /><div className="top-copy"><h3>{clip.title}</h3>
          <a className="channel-link" href={landingLink(`https://www.twitch.tv/${encodeURIComponent(clip.channel)}`)} target="_blank" rel="noopener noreferrer">{clip.channel_name || clip.channel}</a>
          {actions(clip)}</div>
      </article>) : <div className="empty-card empty-card--podium"><span className="empty-trophy" aria-hidden="true"><Trophy size={26} /></span><h3>{landingText("Noch kein Clip im Rennen.")}</h3><p>{submissionsOpen ? landingText("Reiche den ersten Moment aus einem Partnerkanal ein.") : landingText("Für diesen Monat wurden keine sichtbaren Clips eingereicht.")}</p></div>}</div>
    </section>

    {current.total > 3 && <section className="all-clips"><div className="section-heading section-heading--row">
      <div><p className="section-kicker">{landingText("Weitere Einreichungen")}</p><h2>{current.total}{landingText(" Clips im Rennen")}</h2></div>
      {!submissionsOpen && <span className="votes-left">{votingReason}</span>}
    </div><div className="clip-grid">{others.map(clip => <article className="clip-card" key={clip.id}><ClipPlayer clip={clip} />
      <div className="clip-card__body"><div><h3>{clip.title}</h3><span>{clip.channel_name || clip.channel}</span></div>{actions(clip)}</div>
    </article>)}</div>
      {current.next_offset !== null && <button className="button button--outline more-button" disabled={busy} onClick={() => void loadMore()}>{landingText("Weitere Clips zeigen")}</button>}
    </section>}

    <section className="archive" id="archiv"><div className="section-heading section-heading--row"><div>
      <p className="section-kicker"><Crown size={17} /> Hall of Fame</p><h2>{landingText("Diese Momente bleiben.")}</h2></div>
      {archive.months.length > 0 && <label className="archive-select">{landingText("Monat")}<select value={shownMonth?.month || ''} onChange={event => setSelectedMonth(event.target.value)}>
        {archive.months.map(month => <option key={month.month} value={month.month}>{monthLabel(month.month, month.month_label)}</option>)}
      </select></label>}
    </div>
      {archiveError ? <div className="notice" role="status">{archiveError}</div> : shownMonth ? <div className="archive-month">
        <div className="archive-grid">{shownMonth.winners.map(clip => <article className="archive-card" key={clip.rank}>
          <span className="archive-rank">{clip.rank}{landingText(". Platz")}</span><ClipPlayer clip={clip} /><h3>{clip.title}</h3>
          <span>{clip.channel_name || clip.channel}</span><b>{voteLabel(clip.votes)}</b>
          {session.is_admin && <button className="moderate-button" disabled={busy} onClick={() => void hide(clip.id, true)}>{landingText("Ausblenden")}</button>}
        </article>)}</div>{!shownMonth.winners.length && <div className="empty-card empty-card--archive"><Film size={24} aria-hidden="true" /><p>{landingText("Für diesen Monat werden keine Gewinner-Clips angezeigt.")}</p></div>}
      </div> : <div className="empty-card empty-card--archive"><Film size={24} aria-hidden="true" /><p>{landingText("Die erste Hall of Fame entsteht am nächsten Monatsersten.")}</p></div>}
      {archive.next_before && <button className="button button--outline more-button" disabled={busy} onClick={() => void olderMonths()}>{landingText("Ältere Monate laden")}</button>}
    </section>

    {session.is_admin && hidden.length > 0 && <section className="moderation"><h2>{landingText("Ausgeblendete Clips")}</h2>
      <p>{landingText("Stimmen und abgeschlossene Plätze bleiben gespeichert. Jede Änderung wird protokolliert.")}</p>
      {hidden.map(clip => <div className="moderation-row" key={clip.id}><span>{clip.title} · {voteLabel(clip.votes)}</span>
        <button className="button button--outline" disabled={busy} onClick={() => void hide(clip.id, false)}>{landingText("Wieder anzeigen")}</button></div>)}
    </section>}
    <footer className="clip-footer"><span>{landingText("Deutsche Deadlock Community")}</span><a href={landingLink("/twitch/datenschutz")}>{landingText("Datenschutz")}</a><a href={landingLink("/twitch/impressum")}>{landingText("Impressum")}</a>
      <p>{landingText("Bei gleicher Stimmenzahl gewinnt die frühere Einreichung. Alle Zeiten gelten für Deutschland.")}</p>
    </footer>
  </main>
}

createRoot(document.getElementById('root')!).render(<StrictMode><App /></StrictMode>)
