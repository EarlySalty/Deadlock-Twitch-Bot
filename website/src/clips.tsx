import { StrictMode, useEffect, useMemo, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { Award, CalendarDays, Check, Crown, Flame, LogIn, LogOut, Send, ShieldCheck, Trophy } from 'lucide-react'
import './clips.css'

type Clip = {
  id: number
  clip_id: string
  clip_url: string
  title: string
  thumbnail_url?: string | null
  channel: string
  channel_name?: string | null
  votes: number
  my_vote: boolean
  my_own: boolean
}

type Current = {
  month: string
  month_label: string
  phase: 'submission' | 'voting'
  phase_ends_at: string
  submissions: Clip[]
  top3: Clip[]
  authenticated: boolean
  discord_authenticated: boolean
  limits: { submissions_per_month: number; votes_per_month: number }
}

type Session = {
  authenticated: boolean
  provider?: 'discord' | 'twitch' | null
  display_name?: string | null
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
    headers: init?.body ? { 'content-type': 'application/json', ...(init.headers || {}) } : init?.headers,
    ...init,
  })
  const data = await response.json().catch(() => ({}))
  if (!response.ok) {
    throw new Error(data.message || 'Die Anfrage ist fehlgeschlagen.')
  }
  return data as T
}

function twitchEmbedUrl(clipId: string) {
  const parent = typeof window === 'undefined' ? 'deutsche-deadlock-community.de' : window.location.hostname
  const params = new URLSearchParams({ clip: clipId, parent, autoplay: 'false' })
  return `https://clips.twitch.tv/embed?${params.toString()}`
}

function timeLabel(target: string) {
  const millis = new Date(target).getTime() - Date.now()
  if (millis <= 0) return 'endet jetzt'
  const hours = Math.ceil(millis / 3_600_000)
  const days = Math.floor(hours / 24)
  if (days > 0) return `noch ${days} Tag${days === 1 ? '' : 'e'}`
  return `noch ${hours} Stunde${hours === 1 ? '' : 'n'}`
}

function rankName(rank: number) {
  return rank === 1 ? '1. Platz' : `${rank}. Platz`
}

function ClipPlayer({ clip, compact = false }: { clip: Clip; compact?: boolean }) {
  return (
    <div className={compact ? 'clip-player clip-player--compact' : 'clip-player'}>
      <iframe
        src={twitchEmbedUrl(clip.clip_id)}
        title={`${clip.title} von ${clip.channel_name || clip.channel}`}
        allowFullScreen
        loading="lazy"
      />
    </div>
  )
}

function TopCard({ clip, rank }: { clip: Clip; rank: number }) {
  return (
    <article className={`top-card top-card--${rank}`}>
      <div className="top-rank">
        {rank === 1 ? <Crown size={20} /> : <Award size={18} />}
        <span>{rankName(rank)}</span>
      </div>
      <ClipPlayer clip={clip} />
      <div className="top-copy">
        <strong>{clip.title}</strong>
        <span>{clip.channel_name || clip.channel}</span>
        <b>{clip.votes} Stimmen</b>
      </div>
    </article>
  )
}

function App() {
  const [current, setCurrent] = useState<Current | null>(null)
  const [session, setSession] = useState<Session | null>(null)
  const [archive, setArchive] = useState<ArchiveMonth[]>([])
  const [clipUrl, setClipUrl] = useState('')
  const [notice, setNotice] = useState('')
  const [busy, setBusy] = useState(false)

  const load = async () => {
    const [currentData, sessionData, archiveData] = await Promise.all([
      api<Current>('/clips/api/current'),
      api<Session>('/clips/api/session'),
      api<{ months: ArchiveMonth[] }>('/clips/api/archive'),
    ])
    setCurrent(currentData)
    setSession(sessionData)
    setArchive(archiveData.months)
  }

  useEffect(() => {
    void load().catch((error) => setNotice(error.message))
  }, [])

  const topIds = useMemo(() => new Set(current?.top3.map((clip) => clip.id) || []), [current])

  const submit = async (event: React.FormEvent) => {
    event.preventDefault()
    if (!clipUrl.trim()) return
    setBusy(true)
    setNotice('')
    try {
      await api('/clips/api/submit', { method: 'POST', body: JSON.stringify({ clip_url: clipUrl }) })
      setClipUrl('')
      setNotice('Clip eingereicht. Viel Glück!')
      await load()
    } catch (error) {
      setNotice(error instanceof Error ? error.message : 'Der Clip konnte nicht eingereicht werden.')
    } finally {
      setBusy(false)
    }
  }

  const vote = async (submissionId: number) => {
    setBusy(true)
    setNotice('')
    try {
      const result = await api<{ message: string }>('/clips/api/vote', {
        method: 'POST',
        body: JSON.stringify({ submission_id: submissionId }),
      })
      setNotice(result.message)
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

  const hide = async (submissionId: number) => {
    if (!window.confirm('Diesen Clip ausblenden? Die Stimmen bleiben im Audit erhalten.')) return
    setBusy(true)
    try {
      await api(`/clips/api/admin/submissions/${submissionId}/hide`, {
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

  const submissionPhase = current.phase === 'submission'
  const otherClips = current.submissions.filter((clip) => !topIds.has(clip.id))

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
          <p className="hero-text">
            Reiche einen Deadlock-Clip aus unserem Partnernetzwerk ein. Ab dem 22. stimmt die Community über die besten Momente des Monats ab.
          </p>
          <div className="phase-pill">
            <CalendarDays size={17} />
            <b>{submissionPhase ? 'Einreichungsphase' : 'Abstimmungsphase'}</b>
            <span>{timeLabel(current.phase_ends_at)}</span>
          </div>
        </div>

        <aside className="rules-card">
          <div className="rules-title"><ShieldCheck size={20} /> So läuft es</div>
          <ol>
            <li><b>1. bis 21.</b><span>Bis zu 3 gültige Twitch-Clips einreichen.</span></li>
            <li><b>22. bis Monatsende</b><span>Bis zu 5 Stimmen mit Discord vergeben.</span></li>
            <li><b>Am 1.</b><span>Die Top 3 wandern dauerhaft ins Archiv.</span></li>
          </ol>
        </aside>
      </section>

      <section className="submission-panel">
        <div>
          <p className="section-kicker">{submissionPhase ? 'Jetzt einreichen' : 'Einreichungen geschlossen'}</p>
          <h2>{submissionPhase ? 'Dein Clip für ' + current.month_label : 'Jetzt wird abgestimmt'}</h2>
          <p>
            {submissionPhase
              ? 'Der Clip muss höchstens 60 Tage alt sein, Deadlock zeigen und aus einem aktiven Partnerkanal stammen.'
              : 'Während der Abstimmung sind keine neuen Einreichungen möglich.'}
          </p>
        </div>

        {submissionPhase && !session.authenticated ? (
          <div className="login-actions">
            <a className="button button--gold" href="/clips/auth/discord/login"><LogIn size={18} /> Mit Discord anmelden</a>
            <a className="button button--outline" href="/twitch/auth/login?next=%2Fclips"><LogIn size={18} /> Mit Twitch anmelden</a>
          </div>
        ) : submissionPhase ? (
          <form className="submit-form" onSubmit={submit}>
            <label htmlFor="clip-url">Twitch-Clip-Adresse</label>
            <div className="input-row">
              <input
                id="clip-url"
                type="url"
                required
                placeholder="https://clips.twitch.tv/..."
                value={clipUrl}
                onChange={(event) => setClipUrl(event.target.value)}
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
          <div className="vote-status">
            <Check size={18} />
            <span>{session.can_vote ? `${session.votes_limit - session.votes_used} Stimmen übrig` : 'Abstimmungsberechtigung geprüft'}</span>
          </div>
        )}
      </section>

      {notice ? <div className="notice" role="status">{notice}</div> : null}

      <section className="top-section">
        <div className="section-heading">
          <p className="section-kicker"><Trophy size={16} /> Aktuelle Top 3</p>
          <h2>{current.month_label}</h2>
        </div>
        {current.top3.length ? (
          <div className="top-grid">
            {current.top3.map((clip, index) => <TopCard key={clip.id} clip={clip} rank={index + 1} />)}
          </div>
        ) : (
          <div className="empty-card">Noch keine Clips für diesen Monat. Der erste Platz ist frei.</div>
        )}
      </section>

      <section className="all-clips">
        <div className="section-heading section-heading--row">
          <div>
            <p className="section-kicker">Alle Einreichungen</p>
            <h2>{current.submissions.length} Clips im Rennen</h2>
          </div>
          {current.phase === 'voting' && (
            <span className="votes-left">{session.discord_authenticated ? `${session.votes_limit - session.votes_used} Stimmen übrig` : 'Discord-Login zum Abstimmen'}</span>
          )}
        </div>

        <div className="clip-grid">
          {otherClips.map((clip) => (
            <article className="clip-card" key={clip.id}>
              <ClipPlayer clip={clip} compact />
              <div className="clip-card__body">
                <div>
                  <strong>{clip.title}</strong>
                  <span>{clip.channel_name || clip.channel}</span>
                </div>
                <div className="clip-card__footer">
                  <b>{clip.votes} Stimmen</b>
                  {current.phase === 'voting' && (
                    <button
                      type="button"
                      className={clip.my_vote ? 'vote-button vote-button--done' : 'vote-button'}
                      disabled={busy || !session.can_vote || clip.my_vote || clip.my_own}
                      onClick={() => void vote(clip.id)}
                      title={clip.my_own ? 'Eigene Clips können nicht gewählt werden.' : undefined}
                    >
                      {clip.my_vote ? <><Check size={16} /> Gewählt</> : clip.my_own ? 'Dein Clip' : 'Stimme geben'}
                    </button>
                  )}
                  {session.is_admin ? (
                    <button className="moderate-button" type="button" disabled={busy} onClick={() => void hide(clip.id)}>Ausblenden</button>
                  ) : null}
                </div>
              </div>
            </article>
          ))}
        </div>
      </section>

      <section className="archive" id="archiv">
        <div className="section-heading">
          <p className="section-kicker"><Crown size={16} /> Hall of Fame</p>
          <h2>Die Gewinner vergangener Monate</h2>
        </div>
        {archive.length ? archive.map((month) => (
          <div className="archive-month" key={month.month}>
            <h3>{month.month_label}</h3>
            <div className="archive-grid">
              {month.winners.map((clip) => (
                <article className="archive-card" key={`${month.month}-${clip.rank}`}>
                  <span className="archive-rank">{rankName(clip.rank)}</span>
                  <ClipPlayer clip={clip} compact />
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
