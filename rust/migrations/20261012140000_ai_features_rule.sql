-- Multi-Server Paket K (Plan 2026-10-12, Abschnitt 4.9, E14): KI-Schalter pro
-- Discord-Server.
--
-- Regel als eine Funktion. Der LLM-Eingang (`tb_llm::ai_guard`) fragt sie
-- vor jedem Aufruf mit Streamerbezug; globale Sammeljobs filtern ihre
-- Eingangsdaten damit. So gibt es die Regel genau einmal.
--
-- - Streamer ohne `streamer_guilds`-Zeile gehören wie bei Raids und Werbung
--   zum deutschen Server: es gilt dessen Schalter (fehlt die Zeile: an).
--   Unbekannte Logins ebenso. Der heutige Betrieb bleibt damit gleich.
-- - Mit aktiver Mitgliedschaft (Zeile aktiv, Server nicht entfernt): an,
--   sobald einer dieser Server `ai_features` an hat.
-- - Nur inaktive Zeilen: an, wenn alle bisherigen Server sie an haben.
--   Ehemalige Partner fremder Server bleiben so ohne KI.
--
-- Der Ledger-Beleg folgt in `20261012140100_llm_usage_streamer.sql`.

CREATE OR REPLACE FUNCTION twitch_streamer_ai_allowed(
    p_twitch_user_id text,
    p_channel_login  text DEFAULT NULL
) RETURNS boolean
LANGUAGE sql
STABLE
AS $$
    WITH s AS (
        SELECT COALESCE(
            NULLIF(BTRIM(p_twitch_user_id), ''),
            (SELECT i.twitch_user_id
               FROM twitch_streamer_identities i
              WHERE LOWER(i.twitch_login) = LOWER(BTRIM(p_channel_login))
              ORDER BY i.twitch_user_id
              LIMIT 1)
        ) AS twitch_user_id
    ),
    m AS (
        SELECT (sg.active AND g.status = 'active' AND g.removed_at IS NULL) AS aktiv,
               g.ai_features
          FROM s
          JOIN streamer_guilds sg ON sg.twitch_user_id = s.twitch_user_id
          JOIN discord_guilds g ON g.guild_id = sg.guild_id
    )
    SELECT CASE
        WHEN NOT EXISTS (SELECT 1 FROM m) THEN
            COALESCE(
                (SELECT c.ai_features FROM discord_guilds c
                  WHERE c.guild_id = 1289721245281292288),
                true)
        WHEN EXISTS (SELECT 1 FROM m WHERE aktiv) THEN
            EXISTS (SELECT 1 FROM m WHERE aktiv AND ai_features)
        ELSE
            NOT EXISTS (SELECT 1 FROM m WHERE NOT ai_features)
    END
$$;
