-- Seed fuer den Owner-Login, den 20260806120000_social_media_partner_access
-- per FK voraussetzt. Die spaetere Datei ist auf Prod bereits angewandt und
-- deshalb eingefroren: jede Aenderung dort aendert die sqlx-Checksum und
-- bricht den naechsten Start mit "previously applied but has been modified".
--
-- Diese Datei liegt bewusst VOR 20260806120000. sqlx wendet ausstehende
-- Migrationen in Versionsreihenfolge an; nur so existiert die Zeile, wenn
-- der eingefrorene INSERT in social_media_partner_access laeuft.
-- Auf Prod ist earlysalty laengst vorhanden. Der INSERT ist deshalb
-- idempotent (ON CONFLICT DO NOTHING) und wird dort zum No-op, auch wenn
-- sqlx die Datei nachtraeglich als ausstehende aeltere Version nachzieht.
--
-- Kein Schema-Change, nur der fehlende Seed. twitch_user_id bleibt NULL:
-- die Identitaet kommt aus dem Live-Roster, nicht aus dieser Migration.

INSERT INTO public.twitch_streamers (twitch_login)
VALUES ('earlysalty')
ON CONFLICT (twitch_login) DO NOTHING;
