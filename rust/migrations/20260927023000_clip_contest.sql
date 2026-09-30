CREATE TABLE IF NOT EXISTS public.twitch_clip_contest_sessions (
    session_hash TEXT PRIMARY KEY,
    discord_user_id TEXT NOT NULL,
    discord_name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS twitch_clip_contest_sessions_expires_idx
    ON public.twitch_clip_contest_sessions (expires_at);

CREATE TABLE IF NOT EXISTS public.twitch_clip_contest_submissions (
    id BIGSERIAL PRIMARY KEY,
    contest_month DATE NOT NULL,
    clip_db_id BIGINT,
    twitch_clip_id TEXT NOT NULL,
    clip_url TEXT NOT NULL,
    clip_title TEXT NOT NULL,
    clip_thumbnail_url TEXT,
    broadcaster_twitch_id TEXT NOT NULL,
    broadcaster_login TEXT NOT NULL,
    broadcaster_name TEXT,
    game_id TEXT NOT NULL,
    clip_created_at TIMESTAMPTZ NOT NULL,
    submitter_provider TEXT NOT NULL,
    submitter_user_id TEXT NOT NULL,
    submitter_person_key TEXT NOT NULL,
    submitter_display_name TEXT NOT NULL,
    submitted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    hidden_at TIMESTAMPTZ,
    hidden_by TEXT,
    CONSTRAINT twitch_clip_contest_month_first_chk CHECK (EXTRACT(DAY FROM contest_month) = 1),
    CONSTRAINT twitch_clip_contest_submitter_provider_chk CHECK (submitter_provider IN ('discord','twitch')),
    CONSTRAINT twitch_clip_contest_month_clip_uniq UNIQUE (contest_month, twitch_clip_id)
);

CREATE INDEX IF NOT EXISTS twitch_clip_contest_submissions_month_visible_idx
    ON public.twitch_clip_contest_submissions (contest_month, submitted_at, id)
    WHERE hidden_at IS NULL;

CREATE INDEX IF NOT EXISTS twitch_clip_contest_submissions_person_month_idx
    ON public.twitch_clip_contest_submissions (submitter_person_key, contest_month);

CREATE TABLE IF NOT EXISTS public.twitch_clip_contest_votes (
    id BIGSERIAL PRIMARY KEY,
    submission_id BIGINT NOT NULL REFERENCES public.twitch_clip_contest_submissions(id) ON DELETE RESTRICT,
    voter_discord_id TEXT NOT NULL,
    contest_month DATE NOT NULL,
    voted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT twitch_clip_contest_vote_once_uniq UNIQUE (submission_id, voter_discord_id),
    CONSTRAINT twitch_clip_contest_vote_month_first_chk CHECK (EXTRACT(DAY FROM contest_month) = 1)
);

CREATE INDEX IF NOT EXISTS twitch_clip_contest_votes_voter_month_idx
    ON public.twitch_clip_contest_votes (voter_discord_id, contest_month, voted_at);

CREATE INDEX IF NOT EXISTS twitch_clip_contest_votes_submission_idx
    ON public.twitch_clip_contest_votes (submission_id);

CREATE TABLE IF NOT EXISTS public.twitch_clip_contest_hall_of_fame (
    contest_month DATE NOT NULL,
    rank SMALLINT NOT NULL,
    submission_id BIGINT NOT NULL REFERENCES public.twitch_clip_contest_submissions(id) ON DELETE RESTRICT,
    vote_count INTEGER NOT NULL,
    finalized_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (contest_month, rank),
    CONSTRAINT twitch_clip_contest_hof_rank_chk CHECK (rank BETWEEN 1 AND 3),
    CONSTRAINT twitch_clip_contest_hof_submission_uniq UNIQUE (contest_month, submission_id)
);

-- Übergabevertrag für den separat entstehenden Effort-Engine-PR.
-- Append-only und über source_id idempotent, damit der Consumer später exakt
-- einmal clip_submitted bzw. clip_top3 übernehmen kann.
CREATE TABLE IF NOT EXISTS public.twitch_clip_contest_effort_outbox (
    id BIGSERIAL PRIMARY KEY,
    event_type TEXT NOT NULL,
    partner_login TEXT NOT NULL,
    source_id TEXT NOT NULL UNIQUE,
    occurred_at TIMESTAMPTZ NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    consumed_at TIMESTAMPTZ,
    CONSTRAINT twitch_clip_contest_effort_event_chk CHECK (event_type IN ('clip_submitted','clip_top3'))
);

CREATE INDEX IF NOT EXISTS twitch_clip_contest_effort_pending_idx
    ON public.twitch_clip_contest_effort_outbox (id)
    WHERE consumed_at IS NULL;

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchbot') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON
            public.twitch_clip_contest_sessions,
            public.twitch_clip_contest_submissions,
            public.twitch_clip_contest_votes,
            public.twitch_clip_contest_hall_of_fame,
            public.twitch_clip_contest_effort_outbox
        TO twitchbot;
        GRANT USAGE, SELECT ON SEQUENCE
            public.twitch_clip_contest_submissions_id_seq,
            public.twitch_clip_contest_votes_id_seq,
            public.twitch_clip_contest_effort_outbox_id_seq
        TO twitchbot;
    END IF;

    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchdash') THEN
        GRANT SELECT ON
            public.twitch_clip_contest_sessions,
            public.twitch_clip_contest_submissions,
            public.twitch_clip_contest_votes,
            public.twitch_clip_contest_hall_of_fame,
            public.twitch_clip_contest_effort_outbox
        TO twitchdash;
    END IF;
END $$;
