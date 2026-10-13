CREATE TABLE public.social_ai_skill_versions (
    skill_id TEXT NOT NULL CHECK (skill_id ~ '^[a-z_]{1,64}$'),
    version INTEGER NOT NULL CHECK (version BETWEEN 1 AND 1000000),
    previous_version INTEGER,
    content TEXT NOT NULL CHECK (octet_length(content) BETWEEN 1 AND 16384),
    sha256 TEXT NOT NULL CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    origin TEXT NOT NULL CHECK (origin IN ('bundled', 'learned')),
    summary TEXT NOT NULL CHECK (char_length(summary) <= 1000),
    reason TEXT NOT NULL CHECK (char_length(reason) <= 1000),
    evidence JSONB CHECK (evidence IS NULL OR jsonb_typeof(evidence) = 'object'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by TEXT NOT NULL CHECK (char_length(created_by) BETWEEN 1 AND 200),
    PRIMARY KEY (skill_id, version),
    FOREIGN KEY (skill_id, previous_version) REFERENCES public.social_ai_skill_versions (skill_id, version),
    CHECK ((origin = 'bundled') = (previous_version IS NULL))
);

CREATE TABLE public.social_ai_skill_active (
    skill_id TEXT PRIMARY KEY,
    version INTEGER NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by TEXT NOT NULL CHECK (char_length(updated_by) BETWEEN 1 AND 200),
    FOREIGN KEY (skill_id, version) REFERENCES public.social_ai_skill_versions (skill_id, version)
);

CREATE TABLE public.social_ai_skill_activations (
    id BIGSERIAL PRIMARY KEY,
    skill_id TEXT NOT NULL,
    from_version INTEGER NOT NULL,
    to_version INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('learned', 'rollback')),
    actor TEXT NOT NULL CHECK (char_length(actor) BETWEEN 1 AND 200),
    reason TEXT NOT NULL CHECK (char_length(reason) BETWEEN 1 AND 1000),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (skill_id, from_version) REFERENCES public.social_ai_skill_versions (skill_id, version),
    FOREIGN KEY (skill_id, to_version) REFERENCES public.social_ai_skill_versions (skill_id, version)
);
CREATE INDEX social_ai_skill_activations_skill_idx ON public.social_ai_skill_activations (skill_id, id DESC);

CREATE TABLE public.social_ai_change_proposals (
    id BIGSERIAL PRIMARY KEY,
    title TEXT NOT NULL CHECK (char_length(title) BETWEEN 1 AND 200),
    summary TEXT NOT NULL CHECK (char_length(summary) BETWEEN 1 AND 2000),
    reason TEXT NOT NULL CHECK (char_length(reason) BETWEEN 1 AND 2000),
    evidence TEXT CHECK (char_length(evidence) <= 2000),
    status TEXT NOT NULL DEFAULT 'proposed' CHECK (status IN ('proposed', 'reviewed', 'dismissed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE FUNCTION public.twitch_apply_social_ai_skill_roles() RETURNS void
LANGUAGE plpgsql SET search_path = pg_catalog, public AS $roles$
DECLARE table_name TEXT; role_name TEXT;
BEGIN
    FOREACH table_name IN ARRAY ARRAY['social_ai_skill_versions','social_ai_skill_active','social_ai_skill_activations','social_ai_change_proposals'] LOOP
        IF to_regclass('public.' || table_name) IS NOT NULL THEN
            EXECUTE format('REVOKE ALL ON TABLE public.%I FROM PUBLIC', table_name);
            FOREACH role_name IN ARRAY ARRAY['twitchbot','twitchdash','twitchlegacy','twitchanalysis','twitchaudit','twitchcontest','twitchcollector'] LOOP
                IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = role_name) THEN
                    EXECUTE format('REVOKE ALL ON TABLE public.%I FROM %I', table_name, role_name);
                END IF;
            END LOOP;
            FOREACH role_name IN ARRAY ARRAY['twitchbot','twitchdash'] LOOP
                IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = role_name) THEN
                    EXECUTE format('GRANT SELECT, INSERT, UPDATE ON TABLE public.%I TO %I', table_name, role_name);
                END IF;
            END LOOP;
        END IF;
    END LOOP;
    FOREACH table_name IN ARRAY ARRAY['social_ai_skill_activations_id_seq','social_ai_change_proposals_id_seq'] LOOP
        IF to_regclass('public.' || table_name) IS NOT NULL THEN
            EXECUTE format('REVOKE ALL ON SEQUENCE public.%I FROM PUBLIC', table_name);
            FOREACH role_name IN ARRAY ARRAY['twitchbot','twitchdash','twitchlegacy','twitchanalysis','twitchaudit','twitchcontest','twitchcollector'] LOOP
                IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = role_name) THEN
                    EXECUTE format('REVOKE ALL ON SEQUENCE public.%I FROM %I', table_name, role_name);
                END IF;
            END LOOP;
            FOREACH role_name IN ARRAY ARRAY['twitchbot','twitchdash'] LOOP
                IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = role_name) THEN
                    EXECUTE format('GRANT USAGE ON SEQUENCE public.%I TO %I', table_name, role_name);
                END IF;
            END LOOP;
        END IF;
    END LOOP;
END
$roles$;
REVOKE ALL ON FUNCTION public.twitch_apply_social_ai_skill_roles() FROM PUBLIC;
SELECT public.twitch_apply_social_ai_skill_roles();
