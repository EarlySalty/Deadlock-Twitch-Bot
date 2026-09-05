DO $$
BEGIN
    IF to_regclass('public.minimax_usage') IS NOT NULL
       AND (SELECT relkind FROM pg_class WHERE oid = to_regclass('public.minimax_usage')) = 'r'
       AND to_regclass('public.llm_usage') IS NULL THEN
        ALTER TABLE public.minimax_usage RENAME TO llm_usage;
    END IF;
    IF to_regclass('public.idx_mmu_ts') IS NOT NULL
       AND to_regclass('public.idx_llm_usage_ts') IS NULL THEN
        ALTER INDEX public.idx_mmu_ts RENAME TO idx_llm_usage_ts;
    END IF;
    IF to_regclass('public.idx_mmu_source') IS NOT NULL
       AND to_regclass('public.idx_llm_usage_source') IS NULL THEN
        ALTER INDEX public.idx_mmu_source RENAME TO idx_llm_usage_source;
    END IF;
    IF to_regclass('public.minimax_usage_id_seq') IS NOT NULL
       AND to_regclass('public.llm_usage_id_seq') IS NULL THEN
        ALTER SEQUENCE public.minimax_usage_id_seq RENAME TO llm_usage_id_seq;
    END IF;
END $$;

CREATE OR REPLACE VIEW public.minimax_usage AS SELECT * FROM public.llm_usage;

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchbot') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON public.minimax_usage TO twitchbot;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchdash') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON public.minimax_usage TO twitchdash;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchlegacy') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON public.minimax_usage TO twitchlegacy;
    END IF;
END $$;
