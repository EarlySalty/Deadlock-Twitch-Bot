-- Monatlicher Effort-Sieger und zeitlich begrenzter Raid-Ziel-Boost.
--
-- Die laufende Effort-Engine bleibt Eigentümer von partner_effort_events.
-- Hier werden nur unveränderliche Monats-Snapshots und der daraus folgende
-- Raid-Boost persistiert.

CREATE TABLE IF NOT EXISTS public.twitch_partner_effort_season_closures (
    season_key          TEXT PRIMARY KEY,
    season_started_at   TIMESTAMPTZ NOT NULL,
    season_ended_at     TIMESTAMPTZ NOT NULL,
    closed_at           TIMESTAMPTZ NOT NULL,
    CHECK (season_key ~ '^[0-9]{4}-[0-9]{2}$'),
    CHECK (season_ended_at > season_started_at)
);

CREATE TABLE IF NOT EXISTS public.twitch_partner_effort_season_results (
    season_key          TEXT NOT NULL
                        REFERENCES public.twitch_partner_effort_season_closures(season_key)
                        ON DELETE RESTRICT,
    twitch_user_id      TEXT NOT NULL,
    twitch_login        TEXT NOT NULL DEFAULT '',
    rank                INTEGER NOT NULL CHECK (rank > 0),
    points              BIGINT NOT NULL,
    qualified_invites   BIGINT NOT NULL DEFAULT 0 CHECK (qualified_invites >= 0),
    score_reached_at    TIMESTAMPTZ,
    closed_at           TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (season_key, twitch_user_id),
    UNIQUE (season_key, rank)
);

CREATE INDEX IF NOT EXISTS idx_effort_season_results_user
    ON public.twitch_partner_effort_season_results (twitch_user_id, season_key DESC);

CREATE TABLE IF NOT EXISTS public.twitch_partner_raid_boost_grants (
    id                  BIGSERIAL PRIMARY KEY,
    season_key          TEXT NOT NULL UNIQUE
                        REFERENCES public.twitch_partner_effort_season_closures(season_key)
                        ON DELETE RESTRICT,
    twitch_user_id      TEXT NOT NULL,
    twitch_login        TEXT NOT NULL DEFAULT '',
    multiplier          DOUBLE PRECISION NOT NULL DEFAULT 1.15
                        CHECK (multiplier >= 1.0),
    streams_total       SMALLINT NOT NULL DEFAULT 2
                        CHECK (streams_total > 0),
    streams_remaining   SMALLINT NOT NULL DEFAULT 2
                        CHECK (streams_remaining >= 0 AND streams_remaining <= streams_total),
    granted_at          TIMESTAMPTZ NOT NULL,
    expires_at          TIMESTAMPTZ NOT NULL,
    CHECK (expires_at > granted_at)
);

CREATE INDEX IF NOT EXISTS idx_raid_boost_grants_active
    ON public.twitch_partner_raid_boost_grants
       (twitch_user_id, expires_at DESC, streams_remaining);

CREATE TABLE IF NOT EXISTS public.twitch_partner_raid_boost_streams (
    id                  BIGSERIAL PRIMARY KEY,
    grant_id            BIGINT NOT NULL
                        REFERENCES public.twitch_partner_raid_boost_grants(id)
                        ON DELETE RESTRICT,
    twitch_user_id      TEXT NOT NULL,
    session_id          BIGINT NOT NULL,
    stream_started_at   TIMESTAMPTZ NOT NULL,
    reserved_at         TIMESTAMPTZ NOT NULL,
    stream_ended_at     TIMESTAMPTZ,
    deadlock_seconds    INTEGER,
    qualified           BOOLEAN,
    consumed_at         TIMESTAMPTZ,
    UNIQUE (twitch_user_id, session_id),
    CHECK (deadlock_seconds IS NULL OR deadlock_seconds >= 0),
    CHECK ((consumed_at IS NULL) OR qualified IS TRUE)
);

CREATE INDEX IF NOT EXISTS idx_raid_boost_streams_open
    ON public.twitch_partner_raid_boost_streams (twitch_user_id, session_id)
    WHERE stream_ended_at IS NULL;

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchbot') THEN
        GRANT SELECT, INSERT, UPDATE
            ON public.twitch_partner_effort_season_closures,
               public.twitch_partner_effort_season_results,
               public.twitch_partner_raid_boost_grants,
               public.twitch_partner_raid_boost_streams
            TO twitchbot;
        GRANT USAGE, SELECT
            ON SEQUENCE public.twitch_partner_raid_boost_grants_id_seq,
                        public.twitch_partner_raid_boost_streams_id_seq
            TO twitchbot;
    END IF;

    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchdash') THEN
        GRANT SELECT
            ON public.twitch_partner_effort_season_closures,
               public.twitch_partner_effort_season_results,
               public.twitch_partner_raid_boost_grants,
               public.twitch_partner_raid_boost_streams
            TO twitchdash;
    END IF;
END
$$;
