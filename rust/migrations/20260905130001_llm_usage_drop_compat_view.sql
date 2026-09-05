DO $$
BEGIN
    IF to_regclass('public.minimax_usage') IS NOT NULL
       AND (SELECT relkind FROM pg_class WHERE oid = to_regclass('public.minimax_usage')) = 'v' THEN
        DROP VIEW public.minimax_usage;
    END IF;
END $$;
