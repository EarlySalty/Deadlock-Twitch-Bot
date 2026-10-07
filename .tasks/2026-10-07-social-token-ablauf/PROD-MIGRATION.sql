\set ON_ERROR_STOP on
BEGIN;
LOCK TABLE public._sqlx_migrations IN EXCLUSIVE MODE;
SELECT EXISTS (SELECT 1 FROM public._sqlx_migrations WHERE version = 20261007120000) AS migration_applied \gset
\if :migration_applied
DO $verification$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM public._sqlx_migrations
        WHERE version = 20261007120000 AND success
          AND checksum = decode('8b50646d1227c0276ea13fe20bd6315d9703aaf20b35b268daaad9424b646b6e82981ef65f70f861099aac9032e13ca0', 'hex')
    ) THEN
        RAISE EXCEPTION 'Social connection migration checksum does not match';
    END IF;
END
$verification$;
\else
\ir ../../rust/migrations/20261007120000_social_connection_expiry.sql
INSERT INTO public._sqlx_migrations (version, description, success, checksum, execution_time)
VALUES (20261007120000, 'social connection expiry', TRUE,
    decode('8b50646d1227c0276ea13fe20bd6315d9703aaf20b35b268daaad9424b646b6e82981ef65f70f861099aac9032e13ca0', 'hex'), 0);
\endif
GRANT SELECT, INSERT, UPDATE, DELETE ON public.social_media_platform_auth TO twitchbot;
GRANT SELECT ON public.social_media_platform_auth TO twitchdash;
COMMIT;
