CREATE TABLE twitch_community_announcements (
    broadcaster_id text PRIMARY KEY CHECK (broadcaster_id = '1367527782'),
    revision bigint NOT NULL DEFAULT 0,
    settings jsonb NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT now()
);
-- Identische bisherige Rotation; keine neue Nachricht und keine Aktivierungsänderung.
INSERT INTO twitch_community_announcements (broadcaster_id, settings) VALUES ('1367527782',
'{"revision":0,"enabled":true,"includeGlobalEvent":true,"entries":[
{"text":"Bock auf Scrims? Meld dich bei Leo auf unserem Discord, wenn du mitspielen möchtest :) {invite}","enabled":true,"color":"purple"},
{"text":"Zuschauen ist gut, selber mitmischen auch :) Alles rund um unsere Turniere findest du bei uns im Discord. {invite}","enabled":true,"color":"purple"},
{"text":"Die Solo-Queue hat heute wieder Humor? Auf unserem Discord findest du Leute zum gemeinsamen Zocken :) {invite}","enabled":true,"color":"purple"}
]}'::jsonb);
DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchbot') THEN
        GRANT SELECT ON twitch_community_announcements TO twitchbot;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchdash') THEN
        GRANT SELECT, UPDATE ON twitch_community_announcements TO twitchdash;
    END IF;
END $$;
