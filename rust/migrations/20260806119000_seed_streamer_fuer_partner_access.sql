-- Entsperrt jede frische Datenbank.
--
-- Die naechste Migration, 20260806120000_social_media_partner_access.sql, legt
-- die Tabelle an und seedet in einem Zug `('earlysalty', TRUE)`. Dieser Seed
-- haengt an einem Fremdschluessel auf `twitch_streamers`, und auf einer leeren
-- Datenbank gibt es die Zeile nicht: `sqlx migrate run` bricht dort ab. Das trifft
-- jede frische DB, also auch `fresh_migrations_schema`, den hermetischen Test und
-- `scripts/sqlx-prepare.sh`.
--
-- Die verursachende Migration laesst sich nicht mehr reparieren: sie ist auf Prod
-- angewandt, und jede Aenderung an ihrem Text wuerde die Pruefsumme brechen. sqlx
-- prueft die Pruefsummen aller angewandten Migrationen, bevor es irgendetwas
-- ausfuehrt, ein spaeteres Korrektur-Update kaeme also nie zum Zug.
--
-- Darum steht dieser Seed davor statt danach. sqlx ordnet nach Version und
-- ueberspringt, was schon angewandt ist; eine Reihenfolgepruefung gibt es nicht.
-- Auf Prod ist die Zeile laengst da, die Migration laeuft dort also folgenlos
-- durch und laesst die Pruefsumme der Folgemigration unangetastet.
INSERT INTO public.twitch_streamers (twitch_login)
VALUES ('earlysalty')
ON CONFLICT (twitch_login) DO NOTHING;
