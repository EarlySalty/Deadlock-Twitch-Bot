-- Dedup fuer den Lern-Zeitstrahl.
--
-- Chat-Zeilen erreichen den Lernmodus aus zwei Quellen: dem EventSub-Hook
-- (Partner-Kanaele mit `channel:bot`) und dem anonymen Lern-IRC-Reader, der
-- alle live Deadlock-Kanaele mitliest. Diese Mengen ueberschneiden sich, sobald
-- ein Partner live Deadlock streamt — dann landete bisher jede Nachricht
-- zweimal im Zeitstrahl, mit zwei verschiedenen Insert-Zeitpunkten. Aus einer
-- eigenen Nachricht waeren so zwei Reaktions-Samples entstanden, und der
-- Few-Shot-Stil haette dieselbe Zeile doppelt gewichtet.
--
-- Die Twitch-Message-ID ist in beiden Pfaden dieselbe (EventSub `message_id`,
-- IRCv3-Tag `id`) und damit der richtige Schluessel. Stream-Segmente haben
-- keine und bleiben aussen vor.
CREATE UNIQUE INDEX IF NOT EXISTS uq_engagement_learn_timeline_message
    ON public.twitch_engagement_learn_timeline (channel_login, message_id)
    WHERE message_id IS NOT NULL;
