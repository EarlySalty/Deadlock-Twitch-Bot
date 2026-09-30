import { StrictMode, useEffect, useMemo, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { Award, CalendarDays, Check, Crown, Flame, LogIn, LogOut, Send, ShieldCheck, Trophy } from 'lucide-react'
import './clips.css'

type Clip = {
  id: number
  clip_id: string
  title: string
  channel: string
  channel_name?: string | null
  votes: number
  my_vote: boolean
  my_own: boolean
}

type Current = {
  month_label: string
  phase: 'submission' | 'voting'
  phase_ends_at: string
  submissions: Clip[]
  top3: Clip[]
}

type Session = {
  authenticated: boolean
  discord_authenticated: boolean
  is_admin: boolean
  can_submit: boolean
  submissions_used: number
  submissions_limit: number
  can_vote: boolean
  votes_used: number
  votes_limit: number
  discord_eligibility?: {
    account_age_ok: boolean
    member_age_ok: boolean
    present: boolean
  } | null
}

type ArchiveMonth = {
  month: string
  month_label: string
  winners: Array<Clip & { rank: number }>
}

async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, {
    credentials: 'same-origin',
    ...init,
    headers: init?.body
      ? { 'content-type': 'application/json', ...(init.headers || {}) }
      : init?.headers,
  })
  const data = await response.json().catch(() => ({}))
  if (!response.ok) throw new Error(data.message || 'Die Anfrage ist fehlgeschlagen.')
  return data as T
}

function embedUrl(clipId: string) {
  const parent = window.location.hostname || 'deutsche-deadlock-community.de'
  const query = new URLSearchParams({ clip: clipId, parent, autoplay: 'false' })
  return `https://clips.twitch.tv/embed?${query}`
}

function remainingLabel(target: string) {
  const hours = Math.max(0, Math.ceil((new Date(target).getTime() - Date.now()) / 3_600_000))
  const days = Math.floor(hours / 24)
  return days > 0 ? `noch ${days} Tag${days === 1 ? '' : 'e'}` : `noch ${hours} Stunde${hours === 1 ? '' : 'n'}`
}

function ClipPlayer({ clip }: { clip: Clip }) {
  return (
    <div className="clip-player">
      <iframe
        src={embedUrl(clip.clip_id)}
        title={`${clip.title} von ${clip.channel_name || clip.channel}`}
        loading="lazy"
        allowFullScreen
      />
    </div>
  )
}

function App() {
  const [current, setCurrent] = useState<Current | null>(null)
  const [session, setSession] = useState<Session | null>(null)
  const [archive, setArchive] = useState<ArchiveMonth[]>([])
  const [url, setUrl] = useState('')
  const [notice, setNotice] = useState('')
  const [busy, setBusy] = useState(false)

  const load = async () => {
    const [c, s, a] = await Promise.all([
      api<Current>('/clips/api/current'),
      api<Session>('/clips/api/session'),
      api<{ months: ArchiveMonth[] }>('/clips/api/archive'),
    ])
    setCurrent(c)
    setSession(s)
    setArchive(a.months)
  }

  useEffect(() => {
    void load().catch((error) => setNotice(error.message))
  }, [])

  const topIds = useMemo(() => new Set(current?.top3.map((clip) => clip.id) || []), [current])
  const others = current?.submissions.filter((clip) => !topIds.has(clip.id)) || []

  const submit = async (event: React.FormEvent) => {
    event.preventDefault()
    setBusy(true)
    setNotice('')
    try {
      await api('/clips/api/submit', { method: 'POST', body: JSON.stringify({ clip_url: url }) })
      setUrl('')
      setNotice('Clip eingereicht.')
      await load()
    } catch (error) {
      setNotice(error instanceof Error ? error.message : 'Der Clip konnte nicht eingereicht werden.')
    } finally {
      setBusy(false)
    }
  }

  const vote = async (id: number) => {
    setBusy(true)
    setNotice('')
    try {
      await api('/clips/api/vote', { method: 'POST', body: JSON.stringify({ submission_id: id }) })
      setNotice('Stimme gespeichert.')
      await load()
    } catch (error) {
      setNotice(error instanceof Error ? error.message : 'Die Stimme konnte nicht gespeichert werden.')
    } finally {
      setBusy(false)
    }
  }

  const logout = async () => {
    setBusy(true)
    try {
      await api('/clips/auth/logout', { method: 'POST' })
      await load()
    } finally {
      setBusy(false)
    }
  }

  const hide = async (id: number) => {
    if (!window.confirm('Clip ausblenden? Die Stimmen bleiben erhalten.')) return
    setBusy(true)
    try {
      await api(`/clips/api/admin/submissions/${id}/hide`, {
        method: 'POST',
        body: JSON.stringify({ hidden: true }),
      })
      await load()
    } catch (error) {
      setNotice(error instanceof Error ? error.message : 'Der Clip konnte nicht ausgeblendet werden.')
    } finally {
      setBusy(false)
    }
  }

  if (!current || !session) {
    return <main className="clips-page"><div className="loading-card">Clip-Wettbewerb wird geladen …</div></main>
  }

  const submissionsOpen = current.phase === 'submission'
  const votesLeft = Math.max(0, session.votes_limit - session.votes_used)

  return (
    <main className="clips-page">
      <header className="clip-header">
        <a className="brand-mark" href="/" aria-label="Deutsche Deadlock Community">
          <span className="brand-d">D</span>
          <span><b>Deutsche Deadlock</b><small>Community</small></span>
        </a>
        <nav>
          <a href="#aktuell">Aktuell</a>
          <a href="#archiv">Archiv</a>
          {session.discord_authenticated ? (
            <button className="link-button" type="button" onClick={() => void logout()} disabled={busy}>
              <LogOut size={15} /> Abmelden
            </button>
          ) : null}
        </nav>
      </header>

      <section className="hero" id="aktuell">
        <div className="hero-copy">
          <p className="eyebrow"><Flame size={16} /> Clip des Monats</p>
          <h1>Ein Clip. Eine Community.<br /><span>Drei Plätze.</span></h1>
          <p className="hero-text">Deadlock-Momente aus unserem Partnernetzwerk. Bis zum 21. einreichen, ab dem 22. entscheidet die Community.</p>
          <div className="phase-pill">
            <CalendarDays size={17} />
            <b>{submissionsOpen ? 'Einreichungsphase' : 'Abstimmungsphase'}</b>
            <span>{remainingLabel(current.phase_ends_at)}</span>
          </div>
        </div>

        <aside className="rules-card">
          <div className="rules-title"><ShieldCheck size={20} /> So läuft es</div>
          <ol>
            <li><b>1. bis 21.</b><span>Bis zu 3 gültige Twitch-Clips einreichen.</span></li>
            <li><b>22. bis Monatsende</b><span>Bis zu 5 Stimmen mit Discord vergeben.</span></li>
            <li><b>Am 1.</b><span>Die Top 3 wandern dauerhaft in die Hall of Fame.</span></li>
          </ol>
        </aside>
      </section>

      <section className="submission-panel">
        <div>
          <p className="section-kicker">{submissionsOpen ? 'Jetzt einreichen' : 'Einreichungen geschlossen'}</p>
          <h2>{submissionsOpen ? `Dein Clip für ${current.month_label}` : 'Jetzt wird abgestimmt'}</h2>
          <p>{submissionsOpen
            ? 'Der Clip muss höchstens 60 Tage alt sein, Deadlock zeigen und aus einem aktiven Partnerkanal stammen.'
            : 'Zum Abstimmen brauchst du Discord, ein mindestens 30 Tage altes Konto und 7 Tage Mitgliedschaft in der Community.'}</p>
        </div>

        {submissionsOpen && !session.authenticated ? (
          <div className="login-actions">
            <a className="button button--gold" href="/clips/auth/discord/login"><LogIn size={18} /> Mit Discord anmelden</a>
            <a className="button button--outline" href="/twitch/auth/login?next=%2Fclips"><LogIn size={18} /> Mit Twitch anmelden</a>
          </div>
        ) : submissionsOpen ? (
          <form className="submit-form" onSubmit={submit}>
            <label htmlFor="clip-url">Twitch-Clip-Adresse</label>
            <div className="input-row">
              <input
                id="clip-url"
                type="url"
                required
                placeholder="https://clips.twitch.tv/..."
                value={url}
                onChange={(event) => setUrl(event.target.value)}
                disabled={!session.can_submit || busy}
              />
              <button className="button button--gold" type="submit" disabled={!session.can_submit || busy}>
                <Send size={18} /> Einreichen
              </button>
            </div>
            <span className="counter">{session.submissions_used} von {session.submissions_limit} Clips diesen Monat</span>
          </form>
        ) : !session.discord_authenticated ? (
          <a className="button button--gold" href="/clips/auth/discord/login"><LogIn size={18} /> Mit Discord abstimmen</a>
        ) : (
          <div className="vote-status"><Check size={18} /> {votesLeft} Stimmen übrig</div>
        )}
      </section>

      {notice ? <div className="notice" role="status">{notice}</div> : null}

      <section className="top-section">
        <div className="section-heading">
          <p className="section-kicker"><Trophy size={16} /> Aktuelle Top 3</p>
          <h2>{current.month_label}</h2>
        </div>
        <div className="top-grid">
          {current.top3.length ? current.top3.map((clip, index) => (
            <article className={`top-card top-card--${index + 1}`} key={clip.id}>
              <div className="top-rank">
                {index === 0 ? <Crown size={20} /> : <Award size={18} />}
                {index + 1}. Platz
              </div>
              <ClipPlayer clip={clip} />
              <div className="top-copy">
                <strong>{clip.title}</strong>
                <span>{clip.channel_name || clip.channel}</span>
                <b>{clip.votes} Stimmen</b>
              </div>
            </article>
          )) : <div className="empty-card">Noch keine Clips für diesen Monat.</div>}
        </div>
      </section>

      <section className="all-clips">
        <div className="section-heading section-heading--row">
          <div><p className="section-kicker">Alle Einreichungen</p><h2>{current.submissions.length} Clips im Rennen</h2></div>
          {current.phase === 'voting' ? <span className="votes-left">{session.discord_authenticated ? `${votesLeft} Stimmen übrig` : 'Discord-Login zum Abstimmen'}</span> : null}
        </div>

        <div className="clip-grid">
          {others.map((clip) => (
            <article className="clip-card" key={clip.id}>
              <ClipPlayer clip={clip} />
              <div className="clip-card__body">
                <div><strong>{clip.title}</strong><span>{clip.channel_name || clip.channel}</span></div>
                <div className="clip-card__footer">
                  <b>{clip.votes} Stimmen</b>
                  {current.phase === 'voting' ? (
                    <button
                      className={clip.my_vote ? 'vote-button vote-button--done' : 'vote-button'}
                      type="button"
                      disabled={busy || !session.can_vote || clip.my_vote || clip.my_own}
                      onClick={() => void vote(clip.id)}
                    >
                      {clip.my_vote ? <><Check size={16} /> Gewählt</> : clip.my_own ? 'Dein Clip' : 'Stimme geben'}
                    </button>
                  ) : null}
                  {session.is_admin ? <button className="moderate-button" type="button" disabled={busy} onClick={() => void hide(clip.id)}>Ausblenden</button> : null}
                </div>
              </div>
            </article>
          ))}
        </div>
      </section>

      <section className="archive" id="archiv">
        <div className="section-heading">
          <p className="section-kicker"><Crown size={16} /> Hall of Fame</p>
          <h2>Gewinner vergangener Monate</h2>
        </div>
        {archive.length ? archive.map((month) => (
          <div className="archive-month" key={month.month}>
            <h3>{month.month_label}</h3>
            <div className="archive-grid">
              {month.winners.map((clip) => (
                <article className="archive-card" key={`${month.month}-${clip.rank}`}>
                  <span className="archive-rank">{clip.rank}. Platz</span>
                  <ClipPlayer clip={clip} />
                  <strong>{clip.title}</strong>
                  <span>{clip.channel_name || clip.channel}</span>
                  <b>{clip.votes} Stimmen</b>
                </article>
              ))}
            </div>
          </div>
        )) : <div className="empty-card">Die erste Hall of Fame entsteht am nächsten Monatsersten.</div>}
      </section>

      <footer className="clip-footer">
        <span>Deutsche Deadlock Community</span>
        <a href="/twitch/datenschutz">Datenschutz</a>
        <a href="/twitch/impressum">Impressum</a>
      </footer>
    </main>
  )
}

createRoot(document.getElementById('root')!).render(<StrictMode><App /></StrictMode>)
