-- Clip contest: existing dashboard_sessions and twitch_clips_social_media are reused.
-- No OAuth token store, social publisher or effort scoring engine is introduced.
CREATE TABLE public.twitch_clip_contest_months (
    contest_month DATE PRIMARY KEY CHECK (EXTRACT(DAY FROM contest_month) = 1),
    finalized_at TIMESTAMPTZ
);

CREATE FUNCTION public.clip_contest_keep_finalized_month() RETURNS trigger
LANGUAGE plpgsql AS $finalized$
BEGIN
    IF OLD.finalized_at IS NOT NULL THEN
        RAISE EXCEPTION 'Finalized clip contest months are immutable' USING ERRCODE = '55000';
    END IF;
    IF TG_OP = 'DELETE' THEN RETURN OLD; END IF;
    RETURN NEW;
END;
$finalized$;
CREATE TRIGGER clip_month_finalized BEFORE UPDATE OR DELETE ON public.twitch_clip_contest_months
    FOR EACH ROW EXECUTE FUNCTION public.clip_contest_keep_finalized_month();

CREATE TABLE public.twitch_clip_contest_submissions (
    id BIGSERIAL PRIMARY KEY,
    contest_month DATE NOT NULL REFERENCES public.twitch_clip_contest_months(contest_month),
    clip_db_id BIGINT REFERENCES public.twitch_clips_social_media(id) ON DELETE SET NULL,
    twitch_clip_id TEXT NOT NULL,
    clip_url TEXT NOT NULL,
    clip_title TEXT NOT NULL,
    clip_thumbnail_url TEXT,
    broadcaster_twitch_id TEXT NOT NULL,
    broadcaster_login TEXT NOT NULL,
    broadcaster_name TEXT,
    creator_twitch_id TEXT,
    game_id TEXT NOT NULL,
    clip_created_at TIMESTAMPTZ NOT NULL,
    submitter_provider TEXT NOT NULL CHECK (submitter_provider IN ('discord','twitch')),
    submitter_user_id TEXT NOT NULL,
    submitter_person_key TEXT NOT NULL,
    submitter_aliases TEXT[] NOT NULL,
    submitter_display_name TEXT NOT NULL,
    submission_slot SMALLINT NOT NULL CHECK (submission_slot BETWEEN 1 AND 3),
    submitted_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    hidden_at TIMESTAMPTZ,
    hidden_by TEXT,
    UNIQUE (contest_month, twitch_clip_id),
    UNIQUE (contest_month, submitter_person_key, submission_slot),
    UNIQUE (id, contest_month)
);
CREATE INDEX twitch_clip_contest_submissions_visible_idx
    ON public.twitch_clip_contest_submissions (contest_month, submitted_at, id) WHERE hidden_at IS NULL;
CREATE INDEX twitch_clip_contest_submissions_person_idx
    ON public.twitch_clip_contest_submissions (contest_month, submitter_person_key);

CREATE TABLE public.twitch_clip_contest_votes (
    id BIGSERIAL PRIMARY KEY,
    submission_id BIGINT NOT NULL,
    voter_discord_id TEXT NOT NULL,
    contest_month DATE NOT NULL,
    vote_slot SMALLINT NOT NULL CHECK (vote_slot BETWEEN 1 AND 5),
    voted_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    UNIQUE (submission_id, voter_discord_id),
    UNIQUE (contest_month, voter_discord_id, vote_slot),
    FOREIGN KEY (submission_id, contest_month)
        REFERENCES public.twitch_clip_contest_submissions(id, contest_month) ON DELETE RESTRICT
);
CREATE INDEX twitch_clip_contest_votes_submission_idx ON public.twitch_clip_contest_votes (submission_id);

CREATE TABLE public.twitch_clip_contest_hall_of_fame (
    contest_month DATE NOT NULL,
    rank SMALLINT NOT NULL CHECK (rank BETWEEN 1 AND 3),
    submission_id BIGINT NOT NULL,
    vote_count INTEGER NOT NULL CHECK (vote_count >= 0),
    finalized_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (contest_month, rank),
    UNIQUE (contest_month, submission_id),
    FOREIGN KEY (submission_id, contest_month)
        REFERENCES public.twitch_clip_contest_submissions(id, contest_month) ON DELETE RESTRICT
);

CREATE TABLE public.twitch_clip_contest_moderation (
    id BIGSERIAL PRIMARY KEY,
    submission_id BIGINT NOT NULL REFERENCES public.twitch_clip_contest_submissions(id) ON DELETE RESTRICT,
    actor TEXT NOT NULL,
    hidden BOOLEAN NOT NULL,
    reason TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()
);

-- Field names match sync_clip_outbox in the partner effort engine (prompt 2).
-- IDs are the source of truth; the login is a compatibility snapshot for its reader.
CREATE TABLE public.twitch_clip_contest_effort_outbox (
    id BIGSERIAL PRIMARY KEY,
    event_type TEXT NOT NULL CHECK (event_type IN ('clip_submitted','clip_top3')),
    partner_twitch_user_id TEXT NOT NULL,
    streamer_login TEXT NOT NULL,
    source_id TEXT NOT NULL UNIQUE,
    occurred_at TIMESTAMPTZ NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb
);

CREATE FUNCTION public.clip_contest_append_only() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'Clip contest audit rows are append-only' USING ERRCODE = '55000';
END;
$$;
CREATE TRIGGER clip_votes_append_only BEFORE UPDATE OR DELETE ON public.twitch_clip_contest_votes
    FOR EACH ROW EXECUTE FUNCTION public.clip_contest_append_only();
CREATE TRIGGER clip_hof_append_only BEFORE UPDATE OR DELETE ON public.twitch_clip_contest_hall_of_fame
    FOR EACH ROW EXECUTE FUNCTION public.clip_contest_append_only();
CREATE TRIGGER clip_moderation_append_only BEFORE UPDATE OR DELETE ON public.twitch_clip_contest_moderation
    FOR EACH ROW EXECUTE FUNCTION public.clip_contest_append_only();
CREATE TRIGGER clip_effort_append_only BEFORE UPDATE OR DELETE ON public.twitch_clip_contest_effort_outbox
    FOR EACH ROW EXECUTE FUNCTION public.clip_contest_append_only();

DO $$
DECLARE service_role TEXT; sequence_name TEXT;
BEGIN
    FOREACH service_role IN ARRAY ARRAY['twitchbot','twitchdash'] LOOP
        IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = service_role) THEN
            EXECUTE format('GRANT SELECT, INSERT ON public.twitch_clip_contest_months,
                public.twitch_clip_contest_submissions, public.twitch_clip_contest_votes,
                public.twitch_clip_contest_hall_of_fame, public.twitch_clip_contest_moderation,
                public.twitch_clip_contest_effort_outbox TO %I', service_role);
            EXECUTE format('GRANT UPDATE (finalized_at) ON public.twitch_clip_contest_months TO %I', service_role);
            EXECUTE format('GRANT UPDATE (hidden_at, hidden_by) ON public.twitch_clip_contest_submissions TO %I', service_role);
            EXECUTE format('GRANT USAGE, SELECT ON SEQUENCE public.twitch_clip_contest_submissions_id_seq,
                public.twitch_clip_contest_votes_id_seq, public.twitch_clip_contest_moderation_id_seq,
                public.twitch_clip_contest_effort_outbox_id_seq TO %I', service_role);
            -- Existing ClipRepository registers validated clips as pending, never approved.
            EXECUTE format('GRANT INSERT (twitch_login, twitch_user_id) ON public.twitch_streamers TO %I', service_role);
            EXECUTE format('GRANT INSERT (clip_id, clip_url, clip_title, clip_thumbnail_url,
                streamer_login, twitch_user_id, created_at, duration_seconds, view_count,
                game_name, game_id, category_key, status, vod_id, vod_offset_s)
                ON public.twitch_clips_social_media TO %I', service_role);
            EXECUTE format('GRANT UPDATE (layout_override_json) ON public.twitch_clips_social_media TO %I', service_role);
            FOREACH sequence_name IN ARRAY ARRAY[
                pg_get_serial_sequence('public.twitch_clips_social_media', 'id'),
                pg_get_serial_sequence('public.twitch_streamers', 'id')
            ] LOOP
                IF sequence_name IS NOT NULL THEN
                    EXECUTE format('GRANT USAGE, SELECT ON SEQUENCE %s TO %I', sequence_name, service_role);
                END IF;
            END LOOP;
        END IF;
    END LOOP;
END $$;
