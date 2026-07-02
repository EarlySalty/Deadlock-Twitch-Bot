-- Rust schema ownership sentinel.
-- Rust setzt diesen Marker nach erfolgreicher Migration und prueft ihn beim Startup.
-- Der Python-Start-Guard ist Ops/Python-Scope und liest diesen Marker spaeter.

CREATE TABLE IF NOT EXISTS public.tb_schema_ownership (
    component TEXT PRIMARY KEY,
    schema_owner TEXT NOT NULL,
    marker_version INTEGER NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    details_json JSONB NOT NULL DEFAULT '{}'::jsonb
);

INSERT INTO public.tb_schema_ownership (
    component,
    schema_owner,
    marker_version,
    updated_at,
    details_json
) VALUES (
    'analytics_schema',
    'rust',
    1,
    now(),
    '{"set_by":"rust_migration"}'::jsonb
)
ON CONFLICT (component) DO NOTHING;
