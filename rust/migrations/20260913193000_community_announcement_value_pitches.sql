UPDATE twitch_community_announcements
SET settings = jsonb_set(
        settings,
        '{entries}',
        '[
          {"text":"Solo Queue muss nicht Standard sein: bei uns laufen aktive Voice-Lanes für gemeinsame Deadlock-Runden. {invite}","enabled":true,"color":"purple"},
          {"text":"Scrim ohne Gegnerteam? Bei uns kannst du gezielt andere Teams suchen, statt einzelne Leute per DM abzuklappern. {invite}","enabled":true,"color":"purple"},
          {"text":"Ranked reicht nicht mehr? Für unsere Deadlock-Turniere kannst du dich direkt als Teilnehmer anmelden. {invite}","enabled":true,"color":"purple"},
          {"text":"Festgefahren? Bei uns geht ein Deadlock-Coach kostenlos mit dir durchs Replay. {invite}","enabled":true,"color":"purple"},
          {"text":"Keine Lust, Valve-Changelogs selbst zu übersetzen? Neue Deadlock-Patches gibt''s bei uns direkt auf Deutsch. {invite}","enabled":true,"color":"purple"},
          {"text":"Fake-Server und dubiose Service-Pitches im Chat? Unser Bot warnt in Partner-Chats vor typischen Scam-Versuchen. {invite}","enabled":true,"color":"purple"}
        ]'::jsonb,
        true
    ),
    revision = revision + 1,
    updated_at = now()
WHERE broadcaster_id = '1367527782';
