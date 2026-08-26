-- Rueckbau der eigenen Uplink-Token-Tabelle.
--
-- `platform_connections` war ein zweiter Token-Speicher fuer dasselbe
-- Twitch-Konto, mit eigenem OAuth-Weg, eigener Verschluesselung und eigenem
-- Refresh-Job neben `twitch_raid_auth`. Zwei Staende fuer ein Konto heisst:
-- einer ist irgendwann der falsche, und beide Refresh-Jobs streiten sich um
-- denselben Refresh-Token, den Twitch bei jeder Nutzung rotiert. Der Uplink
-- liest ab jetzt aus `twitch_raid_auth`; das Scope-Profil `uplink` holt die
-- Chat- und Stream-Key-Rechte dort mit hinein.
--
-- Keine Uebernahme der Zeilen: die Blobs haengen an einer anderen AAD
-- (`platform_connections:<streamer>:<platform>`) und tragen einen kleineren
-- Scope-Satz. Ein umkopierter Blob liesse sich nicht entschluesseln, und ein
-- uebernommener Grant koennte weder Stream-Key noch Follows. Betroffene
-- Streamer sehen im Dashboard "Neu verbinden" und gehen einmal durch den
-- Twitch-Dialog; ihr Raid-Bot laeuft davon unberuehrt weiter.
--
-- Vor dem Loeschen einmal festhalten, wer betroffen ist, damit die Zahl im
-- Journal steht und nicht geraten werden muss.
DO $$
DECLARE
    anzahl BIGINT;
BEGIN
    IF to_regclass('platform_connections') IS NULL THEN
        RETURN;
    END IF;
    EXECUTE 'SELECT count(*) FROM platform_connections' INTO anzahl;
    RAISE NOTICE 'platform_connections-Rueckbau: % Verbindung(en) werden entfernt, betroffene Streamer muessen den Uplink neu verbinden', anzahl;
END
$$;

DROP TABLE IF EXISTS platform_connections;
