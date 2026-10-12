CREATE TABLE public.social_ai_processing_control (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    revision BIGINT NOT NULL DEFAULT 1 CHECK (revision BETWEEN 1 AND 9007199254740991),
    paused BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by TEXT NOT NULL DEFAULT 'system',
    reason TEXT NOT NULL DEFAULT '' CHECK (char_length(reason) <= 1000)
);
INSERT INTO public.social_ai_processing_control (singleton) VALUES (TRUE);

CREATE TABLE public.social_ai_user_limits (
    twitch_user_id TEXT PRIMARY KEY REFERENCES public.twitch_streamers(twitch_user_id) ON DELETE CASCADE,
    revision BIGINT NOT NULL CHECK (revision BETWEEN 1 AND 9007199254740991),
    paused BOOLEAN NOT NULL DEFAULT FALSE,
    monthly_cap_micro_usd BIGINT CHECK (monthly_cap_micro_usd BETWEEN 0 AND 2000000),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by TEXT NOT NULL,
    reason TEXT NOT NULL CHECK (char_length(reason) BETWEEN 1 AND 1000)
);

CREATE TABLE public.social_ai_plan_limit_drafts (
    plan_id TEXT PRIMARY KEY CHECK (char_length(plan_id) BETWEEN 1 AND 128),
    revision BIGINT NOT NULL CHECK (revision BETWEEN 1 AND 9007199254740991),
    monthly_cap_micro_usd BIGINT CHECK (monthly_cap_micro_usd BETWEEN 0 AND 2000000),
    monthly_ai_jobs BIGINT CHECK (monthly_ai_jobs BETWEEN 0 AND 9007199254740991),
    monthly_revisions BIGINT CHECK (monthly_revisions BETWEEN 0 AND 9007199254740991),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by TEXT NOT NULL,
    reason TEXT NOT NULL CHECK (char_length(reason) BETWEEN 1 AND 1000)
);

CREATE INDEX llm_usage_social_ai_owner_idx ON public.llm_usage (social_ai_twitch_user_id, social_ai_job_id, id)
    WHERE purpose = 'social_editor';

CREATE FUNCTION public.twitch_apply_social_ai_limit_roles() RETURNS void
LANGUAGE plpgsql SET search_path = pg_catalog, public AS $roles$
DECLARE table_name TEXT; role_name TEXT;
BEGIN
    FOREACH table_name IN ARRAY ARRAY['social_ai_processing_control','social_ai_user_limits','social_ai_plan_limit_drafts'] LOOP
        IF to_regclass('public.' || table_name) IS NOT NULL THEN
            EXECUTE format('REVOKE ALL ON TABLE public.%I FROM PUBLIC', table_name);
            FOREACH role_name IN ARRAY ARRAY['twitchbot','twitchdash','twitchlegacy','twitchanalysis','twitchaudit','twitchcontest','twitchcollector'] LOOP
                IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = role_name) THEN
                    EXECUTE format('REVOKE ALL ON TABLE public.%I FROM %I', table_name, role_name);
                END IF;
            END LOOP;
            IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchdash') THEN
                EXECUTE format('GRANT SELECT, INSERT, UPDATE ON TABLE public.%I TO twitchdash', table_name);
            END IF;
            IF table_name <> 'social_ai_plan_limit_drafts' THEN
                FOREACH role_name IN ARRAY ARRAY['twitchbot','twitchanalysis','twitchaudit'] LOOP
                    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = role_name) THEN
                        EXECUTE format('GRANT SELECT ON TABLE public.%I TO %I', table_name, role_name);
                    END IF;
                END LOOP;
            END IF;
        END IF;
    END LOOP;
END
$roles$;
REVOKE ALL ON FUNCTION public.twitch_apply_social_ai_limit_roles() FROM PUBLIC;
SELECT public.twitch_apply_social_ai_limit_roles();
