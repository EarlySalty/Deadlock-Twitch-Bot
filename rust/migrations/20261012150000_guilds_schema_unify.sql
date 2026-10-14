ALTER TABLE public.discord_guilds
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ADD COLUMN IF NOT EXISTS updated_by TEXT;

CREATE TABLE IF NOT EXISTS public.server_admin_audit_log (
    id BIGSERIAL PRIMARY KEY,
    guild_id BIGINT NOT NULL CHECK (guild_id > 0),
    actor TEXT NOT NULL,
    action TEXT NOT NULL,
    detail JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(detail) = 'object'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS server_admin_audit_log_guild
    ON public.server_admin_audit_log (guild_id, created_at DESC);
