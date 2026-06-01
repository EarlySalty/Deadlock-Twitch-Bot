-- Geschichtete Soul-Erweiterungen für den Engagement-Bot.
-- Der statische Kern-Soul lebt als Konstante im Code; hier kommen die DYNAMISCHEN
-- Teile rein: einmalig kuratierte Hero-Takes (kind='hero_takes') und mit der Zeit
-- selbst angehängte Anker (kind='anchor', wenn der Bot ein geiles Gespräch hatte
-- oder was Cooles entdeckt). Die Pipeline liest die jeweils neuesten und hängt sie
-- unter den Kern-Soul.
-- Idempotent: CREATE ... IF NOT EXISTS.
CREATE TABLE IF NOT EXISTS twitch_engagement_soul (
    id          BIGSERIAL PRIMARY KEY,
    kind        TEXT NOT NULL,         -- 'hero_takes' | 'anchor'
    content     TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_eng_soul_kind_created
    ON twitch_engagement_soul (kind, created_at DESC);
