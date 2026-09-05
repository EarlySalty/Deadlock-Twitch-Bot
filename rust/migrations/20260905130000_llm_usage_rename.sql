ALTER TABLE public.minimax_usage RENAME TO llm_usage;
ALTER INDEX public.idx_mmu_ts RENAME TO idx_llm_usage_ts;
ALTER INDEX public.idx_mmu_source RENAME TO idx_llm_usage_source;
ALTER SEQUENCE public.minimax_usage_id_seq RENAME TO llm_usage_id_seq;

CREATE VIEW public.minimax_usage AS SELECT * FROM public.llm_usage;

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
