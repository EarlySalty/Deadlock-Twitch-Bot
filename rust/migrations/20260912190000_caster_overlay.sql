-- Gemeinsame Admin-Caster-Szene; bleibt über Deploys und Neustarts erhalten.
CREATE TABLE IF NOT EXISTS twitch_caster_overlay (
    id boolean PRIMARY KEY DEFAULT true CHECK (id),
    revision bigint NOT NULL DEFAULT 0,
    scene jsonb NOT NULL DEFAULT '{"roster":[],"slots":[null,null]}'::jsonb
);
INSERT INTO twitch_caster_overlay (id) VALUES (true) ON CONFLICT DO NOTHING;
