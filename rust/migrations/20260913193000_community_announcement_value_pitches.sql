UPDATE twitch_community_announcements
SET settings = jsonb_set(
        settings,
        '{entries}',
        '[
          {"text":"Keine Lust auf Solo Queue? Im Discord findest du deutschsprachige Mitspieler für gemeinsame Deadlock-Runden. {invite}","enabled":true,"color":"purple"},
          {"text":"Du willst mehr als Ranked? Im Discord findest du Mitspieler und Gegner für Scrims. {invite}","enabled":true,"color":"purple"},
          {"text":"Du willst kompetitives Deadlock spielen? Im Discord findest du die Infos und Anmeldung zu unseren Turnieren. {invite}","enabled":true,"color":"purple"},
          {"text":"Neu in Deadlock oder noch auf Hero-Suche? Im Discord kannst du Fragen stellen, Tipps bekommen und mit anderen zusammen spielen. {invite}","enabled":true,"color":"purple"}
        ]'::jsonb,
        true
    ),
    revision = revision + 1,
    updated_at = now()
WHERE broadcaster_id = '1367527782';
