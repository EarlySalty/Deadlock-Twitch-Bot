-- Ein Rückgruß pro Chatter und Stream.
--
-- Bisher bremste nur ein kanalweiter Cooldown (10 min). Wer im Verlauf eines
-- Streams mehrfach "hi" schrieb, bekam mehrfach denselben Gruß zurück — das
-- liest sich wie ein Bot, der nicht mitzählt.
--
-- Eine Zeile je (Kanal, Chatter), `greeted_at` wird beim Grüßen fortgeschrieben.
-- "Schon begrüßt" heißt: `greeted_at` liegt nach dem Start der offenen Session
-- in `twitch_stream_sessions`. Darum keine `session_id`: die Tabelle wächst
-- nicht mit jedem Stream, und ein Rückgruß im Offline-Chat (der Bot antwortet
-- auch dort) braucht trotzdem eine Sperre — dafür greift ersatzweise ein
-- Zeitfenster, siehe `standard_replies.rs`.
--
-- Kein Fremdschlüssel auf `twitch_streamers`: der Bot grüßt in jedem Kanal, in
-- dem er sitzt, nicht nur in registrierten.
CREATE TABLE IF NOT EXISTS public.twitch_greeted_chatters (
    streamer_login TEXT NOT NULL,
    chatter_login  TEXT NOT NULL,
    chatter_id     TEXT,
    greeted_at     TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (streamer_login, chatter_login)
);

CREATE INDEX IF NOT EXISTS idx_twitch_greeted_chatters_greeted_at
    ON public.twitch_greeted_chatters (greeted_at);
