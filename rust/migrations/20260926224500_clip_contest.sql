-- Öffentlicher monatlicher Clip-Wettbewerb.
-- Stimmen sind append-only. Verstecken eines Clips löscht keine Stimme.
CREATE TABLE IF NOT EXISTS public.twitch_clip_contest_submissions (
    id BIGSERIAL PRIMARY KEY,
    contest_month DATE NOT NULL,
    clip_id TEXT NOT NULL,
    clip_url TEXT NOT NULL,
    clip_title TEXT NOT NULL,
    clip_thumbnail_url TEXT,
    clip_created_at TIMESTAMPTZ NOT NULL,
    game_id TEXT NOT NULL,
    broadcaster_id TEXT NOT NULL,
    streamer_login TEXT NOT NULL,
    submitter_kind TEXT NOT NULL CHECK (submitter_kind IN ('discord', 'twitch')),
    submitter_id TEXT NOT NULL,
    submitter_display_name TEXT,
    submitter_discord_id TEXT,
    submitter_twitch_id TEXT,
    submitted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    hidden_at TIMESTAMPTZ,
    hidden_by TEXT,
    CHECK (contest_month = date_trunc('month', contest_month::timestamp)::date),
    UNIQUE (contest_month, clip_id)
);

CREATE INDEX IF NOT EXISTS twitch_clip_contest_submissions_month_visible_idx
    ON public.twitch_clip_contest_submissions (contest_month, hidden_at, submitted_at, id);
CREATE INDEX IF NOT EXISTS twitch_clip_contest_submissions_submitter_idx
    ON public.twitch_clip_contest_submissions (contest_month, submitter_kind, submitter_id);
CREATE INDEX IF NOT EXISTS twitch_clip_contest_submissions_streamer_idx
    ON public.twitch_clip_contest_submissions (contest_month, streamer_login);

CREATE TABLE IF NOT EXISTS public.twitch_clip_contest_votes (
    id BIGSERIAL PRIMARY KEY,
    contest_month DATE NOT NULL,
    submission_id BIGINT NOT NULL REFERENCES public.twitch_clip_contest_submissions(id),
    voter_discord_id TEXT NOT NULL,
    voter_display_name TEXT,
    voted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (contest_month = date_trunc('month', contest_month::timestamp)::date),
    UNIQUE (submission_id, voter_discord_id)
);

CREATE INDEX IF NOT EXISTS twitch_clip_contest_votes_voter_month_idx
    ON public.twitch_clip_contest_votes (contest_month, voter_discord_id, voted_at);
CREATE INDEX IF NOT EXISTS twitch_clip_contest_votes_submission_idx
    ON public.twitch_clip_contest_votes (submission_id);

CREATE TABLE IF NOT EXISTS public.twitch_clip_contest_results (
    contest_month DATE NOT NULL,
    rank SMALLINT NOT NULL CHECK (rank BETWEEN 1 AND 3),
    submission_id BIGINT NOT NULL REFERENCES public.twitch_clip_contest_submissions(id),
    clip_id TEXT NOT NULL,
    clip_url TEXT NOT NULL,
    clip_title TEXT NOT NULL,
    clip_thumbnail_url TEXT,
    streamer_login TEXT NOT NULL,
    broadcaster_id TEXT NOT NULL,
    vote_count INTEGER NOT NULL CHECK (vote_count >= 0),
    finalized_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (contest_month, rank),
    UNIQUE (contest_month, submission_id)
);

CREATE TABLE IF NOT EXISTS public.twitch_clip_contest_moderation_audit (
    id BIGSERIAL PRIMARY KEY,
    submission_id BIGINT NOT NULL REFERENCES public.twitch_clip_contest_submissions(id),
    action TEXT NOT NULL CHECK (action IN ('hide', 'unhide')),
    actor TEXT NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Übergabe an den parallel entstehenden Effort-Engine-Vertrag. Die Zeilen
-- sind append-only und durch source_id idempotent.
CREATE TABLE IF NOT EXISTS public.twitch_clip_contest_effort_outbox (
    id BIGSERIAL PRIMARY KEY,
    event_type TEXT NOT NULL CHECK (event_type IN ('clip_submitted', 'clip_top3')),
    streamer_login TEXT NOT NULL,
    source_id TEXT NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    UNIQUE (event_type, source_id)
);

CREATE TABLE IF NOT EXISTS public.twitch_clip_contest_discord_sessions (
    session_lookup_key TEXT PRIMARY KEY,
    discord_id TEXT NOT NULL,
    discord_name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS twitch_clip_contest_discord_sessions_expiry_idx
    ON public.twitch_clip_contest_discord_sessions (expires_at);

-- Nutzt den bestehenden Twitch-OAuth-Start und -Callback. Nur die
-- contest-spezifische Session ist getrennt, damit öffentliche Twitch-Nutzer
-- nicht als Dashboard-Partner behandelt werden.
CREATE TABLE IF NOT EXISTS public.twitch_clip_contest_twitch_sessions (
    session_lookup_key TEXT PRIMARY KEY,
    twitch_user_id TEXT NOT NULL,
    twitch_login TEXT NOT NULL,
    display_name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS twitch_clip_contest_twitch_sessions_expiry_idx
    ON public.twitch_clip_contest_twitch_sessions (expires_at);

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchdash') THEN
        GRANT SELECT, INSERT, UPDATE ON public.twitch_clip_contest_submissions TO twitchdash;
        GRANT SELECT, INSERT ON public.twitch_clip_contest_votes TO twitchdash;
        GRANT SELECT, INSERT ON public.twitch_clip_contest_results TO twitchdash;
        GRANT SELECT, INSERT ON public.twitch_clip_contest_moderation_audit TO twitchdash;
        GRANT SELECT, INSERT ON public.twitch_clip_contest_effort_outbox TO twitchdash;
        GRANT SELECT, INSERT, UPDATE, DELETE ON public.twitch_clip_contest_discord_sessions TO twitchdash;
        GRANT SELECT, INSERT, UPDATE, DELETE ON public.twitch_clip_contest_twitch_sessions TO twitchdash;
        GRANT USAGE, SELECT ON SEQUENCE public.twitch_clip_contest_submissions_id_seq TO twitchdash;
        GRANT USAGE, SELECT ON SEQUENCE public.twitch_clip_contest_votes_id_seq TO twitchdash;
        GRANT USAGE, SELECT ON SEQUENCE public.twitch_clip_contest_moderation_audit_id_seq TO twitchdash;
        GRANT USAGE, SELECT ON SEQUENCE public.twitch_clip_contest_effort_outbox_id_seq TO twitchdash;
    END IF;
END
$$;
