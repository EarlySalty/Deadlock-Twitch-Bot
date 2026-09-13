UPDATE twitch_community_announcements
SET settings = jsonb_set(
        settings,
        '{entries}',
        '[
          {"text":"Für gemeinsame Deadlock-Runden findest du im Discord deutschsprachige Mitspieler. {invite}","enabled":true,"color":"purple"},
          {"text":"Ranked ohne festen Stack? Im Discord finden sich Leute für Premades und gemeinsame Competitive-Runden. {invite}","enabled":true,"color":"purple"},
          {"text":"Für Scrims gibt es im Discord Mitspieler und Gegner zum Organisieren. {invite}","enabled":true,"color":"purple"},
          {"text":"Turnierinfos und Anmeldung für unsere Deadlock-Turniere landen gesammelt im Discord. {invite}","enabled":true,"color":"purple"},
          {"text":"Builds, Items und die aktuelle Meta werden im Discord gemeinsam besprochen. {invite}","enabled":true,"color":"purple"},
          {"text":"Bei Hero- und Einsteigerfragen gibt es im Discord Hilfe und konkrete Tipps. {invite}","enabled":true,"color":"purple"},
          {"text":"Kostenloses Coaching und Replay-Feedback kann im Discord angefragt werden. {invite}","enabled":true,"color":"purple"},
          {"text":"Deadlock-Patchnotes und wichtige Änderungen werden im Discord auf Deutsch aufbereitet. {invite}","enabled":true,"color":"purple"},
          {"text":"Scam-Pitches und Fake-Server sind leider real: Partner-Chats bekommen Scam-Schutz, der offizielle Anlaufpunkt bleibt unser Discord. {invite}","enabled":true,"color":"purple"}
        ]'::jsonb,
        true
    ),
    revision = revision + 1,
    updated_at = now()
WHERE broadcaster_id = '1367527782';
