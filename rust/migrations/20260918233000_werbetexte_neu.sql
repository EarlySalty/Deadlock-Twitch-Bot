WITH disabled_entries AS (
    SELECT COALESCE(
        jsonb_agg(jsonb_set(entry, '{enabled}', 'false'::jsonb, true)),
        '[]'::jsonb
    ) AS entries
    FROM twitch_community_announcements a
    CROSS JOIN LATERAL jsonb_array_elements(COALESCE(a.settings->'entries', '[]'::jsonb)) AS entry
    WHERE a.broadcaster_id = '1367527782'
)
UPDATE twitch_community_announcements AS a
SET settings = jsonb_set(
        a.settings,
        '{entries}',
        disabled_entries.entries || '[
          {"text":"Solo Queue würfelt wieder? Im Discord laufen Voice-Lanes für gemeinsame Deadlock-Runden :) {invite}","enabled":true,"color":"purple"},
          {"text":"Replay sieht nach Fragezeichen aus? Kostenloses Deadlock-Coaching gibt dir eine zweite Meinung :) {invite}","enabled":true,"color":"purple"},
          {"text":"Valve-Changelog wieder halber Roman? Die Deadlock-Patchnotes gibt''s bei uns direkt auf Deutsch :) {invite}","enabled":true,"color":"purple"},
          {"text":"Bock auf 6v6 mit Plan statt Zufallsrunde? Turniere und Scrims laufen über unseren Discord :) {invite}","enabled":true,"color":"purple"},
          {"text":"Im Stream ist gerade mehr los als im Match? Unsere Deadlock-Events stehen gesammelt im Discord :) {invite}","enabled":true,"color":"purple"}
        ]'::jsonb,
        true
    ),
    revision = a.revision + 1,
    updated_at = now()
FROM disabled_entries
WHERE a.broadcaster_id = '1367527782';
