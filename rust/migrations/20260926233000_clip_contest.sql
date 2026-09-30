-- Öffentlicher monatlicher Clip-Wettbewerb.
-- Votes werden nie gelöscht. Moderation blendet nur Submissions aus.
CREATE TABLE IF NOT EXISTS public.twitch_clip_contest_sessions (
    session_hash text PRIMARY KEY,
    discord_user_id text NOT NULL,
    discord_name text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    last_seen_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS twitch_clip_contest_sessions_expires_idx
    ON public.twitch_clip_contest_sessions (expires_at);

CREATE TABLE IF NOT EXISTS public.twitch_clip_contest_submissions (
    id bigserial PRIMARY KEY,
    contest_month date NOT NULL,
    clip_db_id bigint,
    twitch_clip_id text NOT NULL,
    clip_url text NOT NULL,
    clip_title text NOT NULL,
    clip_thumbnail_url text,
    broadcaster_twitch_id text NOT NULL,
    broadcaster_login text NOT NULL,
    broadcaster_name text,
    game_id text NOT NULL,
    clip_created_at timestamptz NOT NULL,
    submitter_provider text NOT NULL,
    submitter_user_id text NOT NULL,
    submitter_person_key text NOT NULL,
    submitter_display_name text NOT NULL,
    submitted_at timestamptz NOT NULL DEFAULT now(),
    hidden_at timestamptz,
    hidden_by text,
    CONSTRAINT twitch_clip_contest_month_first_chk
        CHECK (EXTRACT(DAY FROM contest_month) = 1),
    CONSTRAINT twitch_clip_contest_submitter_provider_chk
        CHECK (submitter_provider IN ('discord', 'twitch')),
    CONSTRAINT twitch_clip_contest_month_clip_uniq UNIQUE (contest_month, twitch_clip_id)
);

CREATE INDEX IF NOT EXISTS twitch_clip_contest_submissions_month_visible_idx
    ON public.twitch_clip_contest_submissions (contest_month, submitted_at, id)
    WHERE hidden_at IS NULL;

CREATE INDEX IF NOT EXISTS twitch_clip_contest_submissions_person_month_idx
    ON public.twitch_clip_contest_submissions (submitter_person_key, contest_month);

CREATE TABLE IF NOT EXISTS public.twitch_clip_contest_votes (
    id bigserial PRIMARY KEY,
    submission_id bigint NOT NULL
        REFERENCES public.twitch_clip_contest_submissions(id) ON DELETE RESTRICT,
    voter_discord_id text NOT NULL,
    contest_month date NOT NULL,
    voted_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT twitch_clip_contest_vote_once_uniq UNIQUE (submission_id, voter_discord_id),
    CONSTRAINT twitch_clip_contest_vote_month_first_chk
        CHECK (EXTRACT(DAY FROM contest_month) = 1)
);

CREATE INDEX IF NOT EXISTS twitch_clip_contest_votes_voter_month_idx
    ON public.twitch_clip_contest_votes (voter_discord_id, contest_month, voted_at);

CREATE INDEX IF NOT EXISTS twitch_clip_contest_votes_submission_idx
    ON public.twitch_clip_contest_votes (submission_id);

CREATE TABLE IF NOT EXISTS public.twitch_clip_contest_hall_of_fame (
    contest_month date NOT NULL,
    rank smallint NOT NULL,
    submission_id bigint NOT NULL
        REFERENCES public.twitch_clip_contest_submissions(id) ON DELETE RESTRICT,
    vote_count integer NOT NULL,
    finalized_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (contest_month, rank),
    CONSTRAINT twitch_clip_contest_hof_rank_chk CHECK (rank BETWEEN 1 AND 3),
    CONSTRAINT twitch_clip_contest_hof_submission_uniq UNIQUE (contest_month, submission_id)
);

-- Übergabevertrag zum Effort-Engine-PR: append-only, idempotent via source_id.
-- Der Engine-Consumer kann diese Zeilen nach seinem eigenen Ledger übernehmen.
CREATE TABLE IF NOT EXISTS public.twitch_clip_contest_effort_outbox (
    id bigserial PRIMARY KEY,
    event_type text NOT NULL,
    partner_twitch_user_id text NOT NULL,
    partner_login text NOT NULL,
    points_hint integer NOT NULL,
    source_id text NOT NULL UNIQUE,
    occurred_at timestamptz NOT NULL,
    metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
    consumed_at timestamptz,
    CONSTRAINT twitch_clip_contest_effort_event_chk
        CHECK (event_type IN ('clip_submitted', 'clip_top3'))
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
        GRANT SELECT, INSERT, UPDATE, DELETE ON
            public.twitch_clip_contest_sessions,
            public.twitch_clip_contest_submissions,
            public.twitch_clip_contest_votes,
            public.twitch_clip_contest_hall_of_fame,
            public.twitch_clip_contest_effort_outbox
        TO twitchdash;
        GRANT USAGE, SELECT ON SEQUENCE
            public.twitch_clip_contest_submissions_id_seq,
            public.twitch_clip_contest_votes_id_seq,
            public.twitch_clip_contest_effort_outbox_id_seq
        TO twitchdash;
    END IF;
END $$;
