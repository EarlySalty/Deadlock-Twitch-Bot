DO $category_readiness_roles$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchbot')
       AND to_regclass('public.category_collection_runs') IS NOT NULL
       AND to_regclass('public.category_stream_snapshots') IS NOT NULL
       AND to_regclass('public.category_collector_status') IS NOT NULL THEN
        EXECUTE 'GRANT SELECT ON TABLE public.category_collection_runs, public.category_stream_snapshots, public.category_collector_status TO twitchbot';
    END IF;
END
$category_readiness_roles$;
