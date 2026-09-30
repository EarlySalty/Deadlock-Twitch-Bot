import React, { useCallback, useEffect, useMemo, useState } from 'react'
import type { FormEvent } from 'react'
import { createRoot } from 'react-dom/client'
import { Crown, ExternalLink, LogIn, Trophy, Vote } from 'lucide-react'
import './ddc-design-tokens.css'
import './clips.css'

type Clip = {
  id: number
  clip_id: string
  url: string
  title: string
  thumbnail_url?: string | null
  created_at: string
  streamer: string
  votes: number
  hidden: boolean
  voted_by_me: boolean
}

type Winner = {
  rank: number
  submission_id: number
  clip_id: string
  url: string
  title: string
  thumbnail_url?: string | null
  streamer: string
  votes: number
  finalized_at: string
  hidden: boolean
}

type ArchiveMonth = {
  month: string
  winners: Winner[]
}

type VoteEligibility = {
  eligible: boolean
  account_old_enough: boolean
  member_old_enough: boolean
  present: boolean
  reason: string
}

type ContestState = {
  now: string
  month: string
  phase: 'submission' | 'voting'
  phase_label: string
  submission_ends_at: string
  voting_ends_at: string
  limits: {
    submissions_per_month: number
    votes_per_month: number
  }
  auth: {
    discord: { id: string; name: string } | null
    twitch: { login: string; name: string } | null
    is_admin: boolean
    can_submit: boolean
    submission_count: number
    votes_used: number
    vote_eligibility: VoteEligibility
  }
  top3: Clip[]
  clips: Clip[]
  archive: ArchiveMonth[]
}

type ApiError = {
  error?: string
  message?: string
}

const monthLabel = (month: string) => {
  const [year, monthIndex] = month.split('-').map(Number)
  return new Intl.DateTimeFormat('de-DE', {
    month: 'long',
    year: 'numeric',
  }).format(new Date(year, monthIndex - 1, 1))
}

const dateLabel = (value: string) =>
  new Intl.DateTimeFormat('de-DE', {
    day: '2-digit',
    month: '2-digit',
    year: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  }).format(new Date(value))

const compactDate = (value: string) =>
  new Intl.DateTimeFormat('de-DE', {
    day: '2-digit',
    month: '2-digit',
    year: 'numeric',
  }).format(new Date(value))

async function requestJson<T>(url: string, init?: RequestInit): Promise<T> {
  const response = await fetch(url, {
    credentials: 'same-origin',
    ...init,
    headers: {
      'Content-Type': 'application/json',
      ...(init?.headers ?? {}),
    },
  })
  const payload = (await response.json().catch(() => ({}))) as ApiError & T
  if (!response.ok) {
    throw new Error(payload.message || 'Die Anfrage konnte nicht abgeschlossen werden.')
  }
  return payload as T
}

function ClipEmbed({ clipId, title }: { clipId: string; title: string }) {
  const parent = window.location.hostname
  const src =
    'https://clips.twitch.tv/embed?clip=' +
    encodeURIComponent(clipId) +
    '&parent=' +
    encodeURIComponent(parent) +
    '&autoplay=false'
  return (
    <div className="clip-embed">
      <iframe
        src={src}
        title={title}
        allowFullScreen
        loading="lazy"
        referrerPolicy="strict-origin-when-cross-origin"
      />
    </div>
  )
}

function RankBadge({ rank }: { rank: number }) {
  return (
    <div className={'rank-badge rank-' + rank} aria-label={'Platz ' + rank}>
      {rank === 1 ? <Crown size={18} aria-hidden="true" /> : <Trophy size={17} aria-hidden="true" />}
      <span>#{rank}</span>
    </div>
  )
}

function WinnerCard({ clip, rank }: { clip: Clip | Winner; rank: number }) {
  return (
    <article className={'winner-card winner-' + rank}>
      <div className="winner-head">
        <RankBadge rank={rank} />
        <span className="vote-count">{clip.votes} {clip.votes === 1 ? 'Stimme' : 'Stimmen'}</span>
      </div>
      {clip.hidden ? (
        <div className="moderated-placeholder">
          <span>Dieser Clip wurde moderiert.</span>
        </div>
      ) : (
        <ClipEmbed clipId={clip.clip_id} title={clip.title} />
      )}
      <div className="clip-copy">
        <div className="streamer-name">{clip.streamer}</div>
        <h3>{clip.hidden ? 'Moderierter Clip' : clip.title}</h3>
      </div>
    </article>
  )
}

function ClipCard({
  clip,
  canVote,
  isAdmin,
  onVote,
  onVisibility,
  busy,
}: {
  clip: Clip
  canVote: boolean
  isAdmin: boolean
  onVote: (id: number) => void
  onVisibility: (id: number, hidden: boolean) => void
  busy: string | null
}) {
  const voteBusy = busy === 'vote:' + clip.id
  const hideBusy = busy === 'hide:' + clip.id
  return (
    <article className="clip-card">
      <ClipEmbed clipId={clip.clip_id} title={clip.title} />
      <div className="clip-copy">
        <div className="clip-meta">
          <span>{clip.streamer}</span>
          <span>{compactDate(clip.created_at)}</span>
        </div>
        <h3>{clip.title}</h3>
        <div className="clip-actions">
          <div className="vote-pill">
            <Vote size={16} aria-hidden="true" />
            <strong>{clip.votes}</strong>
          </div>
          {canVote && (
            <button
              className="button button-primary"
              type="button"
              disabled={voteBusy || clip.voted_by_me}
              onClick={() => onVote(clip.id)}
            >
              {clip.voted_by_me ? 'Stimme vergeben' : voteBusy ? 'Wird gespeichert' : 'Abstimmen'}
            </button>
          )}
          <a className="button button-quiet" href={clip.url} target="_blank" rel="noreferrer">
            Twitch <ExternalLink size={15} aria-hidden="true" />
          </a>
          {isAdmin && (
            <button
              className="button button-danger"
              type="button"
              disabled={hideBusy}
              onClick={() => onVisibility(clip.id, true)}
            >
              {hideBusy ? 'Wird ausgeblendet' : 'Ausblenden'}
            </button>
          )}
        </div>
      </div>
    </article>
  )
}

function LoginBox({ state }: { state: ContestState }) {
  const hasDiscord = Boolean(state.auth.discord)
  const hasTwitch = Boolean(state.auth.twitch)
  let loginName = 'Noch nicht angemeldet'
  if (hasDiscord) loginName = 'Discord: ' + state.auth.discord?.name
  else if (hasTwitch) loginName = 'Twitch: ' + state.auth.twitch?.name

  return (
    <div className="login-box">
      <div>
        <span className="eyebrow">Anmeldung</span>
        <strong>{loginName}</strong>
      </div>
      <div className="login-actions">
        {!hasDiscord && (
          <a className="button button-primary" href="/clips/auth/discord">
            <LogIn size={16} aria-hidden="true" />
            Mit Discord anmelden
          </a>
        )}
        {!hasTwitch && (
          <a className="button button-secondary" href="/twitch/auth/login?next=%2Fclips">
            Mit Twitch anmelden
          </a>
        )}
      </div>
    </div>
  )
}

function SubmissionPanel({
  state,
  onSubmit,
  busy,
}: {
  state: ContestState
  onSubmit: (url: string) => void
  busy: string | null
}) {
  const [clipUrl, setClipUrl] = useState('')
  const loggedIn = Boolean(state.auth.discord || state.auth.twitch)
  const remaining = Math.max(0, state.limits.submissions_per_month - state.auth.submission_count)

  const submit = (event: FormEvent) => {
    event.preventDefault()
    if (!clipUrl.trim()) return
    onSubmit(clipUrl.trim())
  }

  return (
    <section className="panel submission-panel" id="einreichen">
      <div className="section-heading compact-heading">
        <div>
          <span className="eyebrow">Dein Clip</span>
          <h2>Für diesen Monat einreichen</h2>
        </div>
        <span className="counter">{remaining} von {state.limits.submissions_per_month} übrig</span>
      </div>
      <p className="muted">
        Der Clip muss aus einem Deadlock-Stream eines aktiven Partnerkanals stammen und darf höchstens 60 Tage alt sein.
      </p>
      {!loggedIn ? (
        <LoginBox state={state} />
      ) : (
        <form className="submit-form" onSubmit={submit}>
          <label htmlFor="clip-url">Twitch-Clip-URL</label>
          <div className="submit-row">
            <input
              id="clip-url"
              type="url"
              inputMode="url"
              placeholder="https://clips.twitch.tv/..."
              value={clipUrl}
              onChange={(event) => setClipUrl(event.target.value)}
              disabled={!state.auth.can_submit || busy === 'submit'}
              required
            />
            <button
              className="button button-primary"
              type="submit"
              disabled={!state.auth.can_submit || busy === 'submit'}
            >
              {busy === 'submit' ? 'Wird geprüft' : 'Clip einreichen'}
            </button>
          </div>
        </form>
      )}
    </section>
  )
}

function VotingStatus({ state }: { state: ContestState }) {
  const eligibility = state.auth.vote_eligibility
  const remaining = Math.max(0, state.limits.votes_per_month - state.auth.votes_used)
  return (
    <section className="panel voting-panel">
      <div>
        <span className="eyebrow">Deine Stimmen</span>
        <h2>{remaining} von {state.limits.votes_per_month} übrig</h2>
      </div>
      {!state.auth.discord ? (
        <div>
          <p className="muted">Zum Abstimmen brauchst du den Discord-Login.</p>
          <a className="button button-primary" href="/clips/auth/discord">
            Mit Discord anmelden
          </a>
        </div>
      ) : eligibility.eligible ? (
        <p className="eligibility-ok">Du bist für die Abstimmung freigeschaltet.</p>
      ) : (
        <p className="eligibility-note">{eligibility.reason}</p>
      )}
    </section>
  )
}

function Archive({ months }: { months: ArchiveMonth[] }) {
  if (!months.length) {
    return (
      <section className="archive-section">
        <div className="section-heading">
          <div>
            <span className="eyebrow">Hall of Fame</span>
            <h2>Die Monatsgewinner</h2>
          </div>
        </div>
        <div className="empty-state">Der erste Monatsabschluss steht noch aus.</div>
      </section>
    )
  }

  return (
    <section className="archive-section" id="archiv">
      <div className="section-heading">
        <div>
          <span className="eyebrow">Hall of Fame</span>
          <h2>Die Monatsgewinner</h2>
        </div>
      </div>
      <div className="archive-list">
        {months.map((month, index) => (
          <details className="archive-month" key={month.month} open={index === 0}>
            <summary>
              <span>{monthLabel(month.month)}</span>
              <span>{month.winners.length} Gewinner</span>
            </summary>
            <div className="podium-grid archive-grid">
              {month.winners.map((winner) => (
                <WinnerCard key={month.month + '-' + winner.rank} clip={winner} rank={winner.rank} />
              ))}
            </div>
          </details>
        ))}
      </div>
    </section>
  )
}

function App() {
  const [state, setState] = useState<ContestState | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [busy, setBusy] = useState<string | null>(null)

  const load = useCallback(async () => {
    try {
      const next = await requestJson<ContestState>('/clips/api/state')
      setState(next)
      setError(null)
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : 'Der Clip-Wettbewerb ist gerade nicht erreichbar.')
    }
  }, [])

  useEffect(() => {
    void load()
  }, [load])

  const canVote = useMemo(() => {
    if (!state || state.phase !== 'voting' || !state.auth.discord) return false
    return state.auth.vote_eligibility.eligible && state.auth.votes_used < state.limits.votes_per_month
  }, [state])

  const submit = async (clipUrl: string) => {
    setBusy('submit')
    setNotice(null)
    try {
      await requestJson<{ ok: boolean }>('/clips/api/submit', {
        method: 'POST',
        body: JSON.stringify({ clip_url: clipUrl }),
      })
      setNotice('Clip eingereicht. Er ist jetzt im Wettbewerb.')
      await load()
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : 'Der Clip konnte nicht eingereicht werden.')
    } finally {
      setBusy(null)
    }
  }

  const vote = async (id: number) => {
    setBusy('vote:' + id)
    setNotice(null)
    try {
      await requestJson<{ ok: boolean }>('/clips/api/vote/' + id, {
        method: 'POST',
        body: '{}',
      })
      setNotice('Deine Stimme wurde gespeichert.')
      await load()
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : 'Die Stimme konnte nicht gespeichert werden.')
    } finally {
      setBusy(null)
    }
  }

  const visibility = async (id: number, hidden: boolean) => {
    setBusy('hide:' + id)
    try {
      await requestJson<{ ok: boolean }>('/clips/api/admin/' + id + '/visibility', {
        method: 'POST',
        body: JSON.stringify({ hidden }),
      })
      setNotice('Der Clip wurde ausgeblendet. Die bisherigen Stimmen bleiben im Prüfverlauf erhalten.')
      await load()
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : 'Die Moderation konnte nicht gespeichert werden.')
    } finally {
      setBusy(null)
    }
  }

  if (!state) {
    return (
      <main className="clips-shell">
        <div className="loading-card">
          <span className="brand-mark">DDC</span>
          <p>{error || 'Clip-Wettbewerb wird geladen.'}</p>
          {error && <button className="button button-primary" onClick={() => void load()}>Erneut versuchen</button>}
        </div>
      </main>
    )
  }

  const votingEnds = dateLabel(state.voting_ends_at)
  const submissionEnds = dateLabel(state.submission_ends_at)

  return (
    <div className="clips-shell">
      <header className="site-header">
        <a className="brand" href="/streamer/">
          <span className="brand-mark">DDC</span>
          <span>
            <strong>Deutsche Deadlock Community</strong>
            <small>Clip des Monats</small>
          </span>
        </a>
        <nav aria-label="Clip-Navigation">
          <a href="#clips">Clips</a>
          <a href="#archiv">Archiv</a>
        </nav>
      </header>

      <main className="content">
        <section className="hero">
          <div className="hero-copy">
            <span className="phase-chip">{state.phase_label}</span>
            <p className="eyebrow">{monthLabel(state.month)}</p>
            <h1>Clip des Monats</h1>
            <p className="hero-lead">
              Reicht eure besten Deadlock-Momente ein und entscheidet gemeinsam, welche drei Clips in die Hall of Fame kommen.
            </p>
            <div className="phase-dates">
              <span>Einreichen bis {submissionEnds}</span>
              <span>Abstimmung endet {votingEnds}</span>
            </div>
          </div>
          <LoginBox state={state} />
        </section>

        {(error || notice) && (
          <div className={error ? 'message message-error' : 'message message-ok'} role="status">
            {error || notice}
            {error && (
              <button type="button" onClick={() => setError(null)} aria-label="Hinweis schließen">
                ×
              </button>
            )}
          </div>
        )}

        <section className="top-section">
          <div className="section-heading">
            <div>
              <span className="eyebrow">{state.phase === 'voting' ? 'Aktueller Stand' : 'Schon dabei'}</span>
              <h2>Die aktuellen Top 3</h2>
            </div>
            <p>
              {state.phase === 'voting'
                ? 'Die Reihenfolge folgt den gültigen Stimmen.'
                : 'Die Abstimmung startet am 22. des Monats.'}
            </p>
          </div>
          {state.top3.length ? (
            <div className="podium-grid">
              {state.top3.map((clip, index) => (
                <WinnerCard key={clip.id} clip={clip} rank={index + 1} />
              ))}
            </div>
          ) : (
            <div className="empty-state">Noch ist kein Clip für diesen Monat dabei.</div>
          )}
        </section>

        {state.phase === 'submission' ? (
          <SubmissionPanel state={state} onSubmit={submit} busy={busy} />
        ) : (
          <VotingStatus state={state} />
        )}

        <section className="clips-section" id="clips">
          <div className="section-heading">
            <div>
              <span className="eyebrow">Alle Beiträge</span>
              <h2>{state.clips.length} Clips im Wettbewerb</h2>
            </div>
            {state.phase === 'voting' && (
              <p>{state.auth.votes_used} von {state.limits.votes_per_month} Stimmen vergeben</p>
            )}
          </div>
          {state.clips.length ? (
            <div className="clips-grid">
              {state.clips.map((clip) => (
                <ClipCard
                  key={clip.id}
                  clip={clip}
                  canVote={canVote}
                  isAdmin={state.auth.is_admin}
                  onVote={vote}
                  onVisibility={visibility}
                  busy={busy}
                />
              ))}
            </div>
          ) : (
            <div className="empty-state">
              {state.phase === 'submission'
                ? 'Noch keine Einreichung. Der erste Clip kann den Monat eröffnen.'
                : 'Für diesen Monat stehen keine sichtbaren Clips zur Abstimmung.'}
            </div>
          )}
        </section>

        <Archive months={state.archive} />
      </main>

      <footer>
        <span>Deutsche Deadlock Community</span>
        <a href="/streamer/">Zur Community</a>
      </footer>
    </div>
  )
}

createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
)
