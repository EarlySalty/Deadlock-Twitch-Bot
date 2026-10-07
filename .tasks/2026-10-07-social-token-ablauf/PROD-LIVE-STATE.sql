\set ON_ERROR_STOP on
BEGIN READ ONLY;
SELECT current_database(), current_user;
SELECT version, success, encode(checksum, 'hex') AS checksum
FROM public._sqlx_migrations
WHERE version IN (20261007120000, 20261007213000);
SELECT platform,
       count(*) FILTER (WHERE enabled = 1) AS active_connections,
       count(*) FILTER (WHERE enabled = 1 AND refresh_token_enc IS NOT NULL) AS renewable_connections
FROM public.social_media_platform_auth
GROUP BY platform ORDER BY platform;
SELECT EXISTS (
    SELECT 1 FROM information_schema.columns
    WHERE table_schema = 'public' AND table_name = 'social_media_platform_auth'
      AND column_name = 'refresh_expires_at'
) AS has_connection_expiry \gset
\if :has_connection_expiry
SELECT platform,
       count(*) FILTER (WHERE enabled = 1 AND refresh_expires_at IS NOT NULL) AS known_refresh_deadlines,
       count(*) FILTER (WHERE enabled = 1 AND needs_reauth) AS reconnect_required
FROM public.social_media_platform_auth
GROUP BY platform ORDER BY platform;
\endif
COMMIT;
