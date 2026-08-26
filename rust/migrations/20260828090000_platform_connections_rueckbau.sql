-- Twitch zieht aus `platform_connections` aus.
--
-- Die Tabelle war ein zweiter Token-Speicher fuer dasselbe Twitch-Konto, mit
-- eigenem OAuth-Weg, eigener Verschluesselung und eigenem Refresh-Job neben
-- `twitch_raid_auth`. Zwei Staende fuer ein Konto heisst: einer ist irgendwann
-- der falsche, und beide Refresh-Jobs streiten sich um denselben
-- Refresh-Token, den Twitch bei jeder Nutzung rotiert. Der Uplink liest fuer
-- Twitch ab jetzt aus `twitch_raid_auth`; das Scope-Profil `uplink` holt die
-- Chat- und Stream-Key-Rechte dort mit hinein.
--
-- Die Tabelle selbst bleibt stehen. Kick, YouTube und TikTok haben keinen
-- Raid-Bot, an dessen Grant sich ein Chat-Zugang anhaengen liesse; fuer sie
-- ist dieser Speicher weiterhin der richtige Ort, und die verschluesselte
-- Ablage samt AAD ist gebaut und geprueft. Sie zu loeschen und spaeter noch
-- einmal zu bauen waere doppelte Arbeit ohne Gegenwert.
--
-- Keine Uebernahme der Twitch-Zeilen nach `twitch_raid_auth`: die Blobs
-- haengen an einer anderen AAD (`platform_connections:<streamer>:<platform>`)
-- und tragen einen kleineren Scope-Satz. Ein umkopierter Blob liesse sich
-- nicht entschluesseln, und ein uebernommener Grant koennte weder Stream-Key
-- noch Follows. Betroffene Streamer sehen im Dashboard "Neu verbinden nötig"
-- und gehen einmal durch den Twitch-Dialog; ihr Raid-Bot laeuft davon
-- unberuehrt weiter.
--
-- Vor dem Loeschen einmal festhalten, wie viele es sind, damit die Zahl im
-- Journal steht und nicht geraten werden muss.
DO $$
DECLARE
    anzahl BIGINT;
BEGIN
    IF to_regclass('platform_connections') IS NULL THEN
        RETURN;
    END IF;
    EXECUTE 'SELECT count(*) FROM platform_connections WHERE platform = ''twitch''' INTO anzahl;
    RAISE NOTICE 'platform_connections-Rueckbau: % Twitch-Verbindung(en) werden entfernt, betroffene Streamer muessen den Uplink neu verbinden', anzahl;
    EXECUTE 'DELETE FROM platform_connections WHERE platform = ''twitch''';
END
$$;
