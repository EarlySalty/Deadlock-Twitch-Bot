ALTER TABLE public.social_ai_jobs
    ADD COLUMN root_job_id BIGINT REFERENCES public.social_ai_jobs(id),
    ADD COLUMN parent_version_id BIGINT,
    ADD COLUMN revision_feedback TEXT CHECK (octet_length(revision_feedback) BETWEEN 1 AND 4000),
    ADD COLUMN revision_proposal JSONB CHECK (jsonb_typeof(revision_proposal) = 'object'),
    ADD COLUMN revision_confirmed BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN revision_reanalysis_confirmed BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN selected_version_id BIGINT,
    ADD CONSTRAINT social_ai_revision_identity CHECK ((root_job_id IS NULL AND parent_version_id IS NULL AND revision_feedback IS NULL) OR (root_job_id IS NOT NULL AND parent_version_id IS NOT NULL AND revision_feedback IS NOT NULL AND root_job_id < id)),
    DROP CONSTRAINT social_ai_jobs_status_check,
    ADD CONSTRAINT social_ai_jobs_status_check CHECK (status IN ('queued', 'preparing', 'transcribing', 'analyzing', 'planning', 'rendering', 'validating', 'review', 'failed', 'awaiting_confirmation', 'rejected'));

DROP INDEX public.social_ai_jobs_active_clip_uq;
CREATE UNIQUE INDEX social_ai_jobs_active_clip_uq ON public.social_ai_jobs (twitch_user_id, clip_db_id)
    WHERE status IN ('queued', 'preparing', 'transcribing', 'analyzing', 'planning', 'rendering', 'validating', 'awaiting_confirmation');
CREATE INDEX social_ai_jobs_root_idx ON public.social_ai_jobs (root_job_id) WHERE root_job_id IS NOT NULL;

CREATE TABLE public.social_ai_revision_events (
    id BIGSERIAL PRIMARY KEY,
    twitch_user_id BIGINT NOT NULL,
    root_job_id BIGINT NOT NULL REFERENCES public.social_ai_jobs(id),
    job_id BIGINT NOT NULL REFERENCES public.social_ai_jobs(id),
    produced_video_id BIGINT,
    event_kind TEXT NOT NULL CHECK (event_kind IN ('feedback', 'proposal_accepted', 'proposal_rejected', 'version_selected', 'version_accepted', 'version_rejected')),
    payload JSONB NOT NULL CHECK (jsonb_typeof(payload) = 'object' AND octet_length(payload::text) <= 65536),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX social_ai_revision_events_root_idx ON public.social_ai_revision_events (twitch_user_id, root_job_id, id);

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchdash') THEN
        GRANT SELECT, INSERT ON public.social_ai_revision_events TO twitchdash;
        GRANT USAGE, SELECT ON SEQUENCE public.social_ai_revision_events_id_seq TO twitchdash;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchbot') THEN
        GRANT SELECT, INSERT ON public.social_ai_revision_events TO twitchbot;
        GRANT USAGE, SELECT ON SEQUENCE public.social_ai_revision_events_id_seq TO twitchbot;
    END IF;
END
$$;
