# Deploy: Lurker-Steuer mit Kanalpunkte-Belohnung

## Migration (nicht von der Session angewandt)

Datei: `rust/migrations/20260909120000_twitch_bot_capabilities.sql`
sha384: `9e1afc4d43314aa56de07f5b0532b5cd7740c387d194268c1eba297ef9563917d56237e740e4926883fe4eee1730863c`

Der Bot migriert nicht selbst (`TB_DB_MIGRATE=0`). Tabelle als `postgres` in der Twitch-DB `twitch_analytics` anlegen, Rechte vergeben, Version von Hand eintragen.

1. Als `postgres` anwenden:

   ```
   psql -d twitch_analytics -f rust/migrations/20260909120000_twitch_bot_capabilities.sql
   ```

   Der DO-Block vergibt bereits `SELECT, INSERT, UPDATE, DELETE` an `twitchbot` und `SELECT` an `twitchdash`, falls die Rollen existieren. Der Bot schreibt die Statuszeile, das Dashboard liest sie.

2. Version in `_sqlx_migrations` als `postgres` nachtragen (Checksumme ist sha384 der Datei als bytea):

   ```
   INSERT INTO _sqlx_migrations
     (version, description, installed_on, success, checksum, execution_time)
   VALUES
     (20260909120000, 'twitch bot capabilities', NOW(), TRUE,
      decode('9e1afc4d43314aa56de07f5b0532b5cd7740c387d194268c1eba297ef9563917d56237e740e4926883fe4eee1730863c','hex'),
      0);
   ```

3. Prüfen: `SELECT * FROM twitch_bot_capabilities;` liefert Zeile `id=1`. Der Chatters-Collect-Loop im Bot upsertet den echten Scope-Wert je 30-s-Tick.

## Service-Neustart nach Merge

- Twitch-Bot und Twitch-Dashboard sind System-Units: `sudo systemctl restart deadlock-twitch-bot-rust deadlock-twitch-dashboard-rust`.

## Laufzeit-Verdrahtung (produktiv, erledigt)

- REQ-5 Reward-Checker ist in `bin/tb-bot` verdrahtet: `build_lurker_reward_checker` baut einen `HelixLurkerRewardChecker` aus `HelixClient` plus dem Streamer-`TokenProvider` (`follower_streamer_token_provider`, Scope `channel:read:redemptions` aus dem Basisprofil) und wird über `ChatRuntimePorts` an die `PromoEngine` gehängt. Nachweis im Journal: `lurker-tax: Reward-Checker verdrahtet` beim Start. Ohne aktive Belohnung sendet die Erinnerung nicht.
- REQ-6 `reward_present` kommt live aus Helix `GET channel_points/custom_rewards` (über `platform_token::gueltiger_twitch_token` mit Streamer-Token und Refresh), Ergebnis 60 s je Login gecacht. Frisch angelegte Belohnung zeigt sofort "Belohnung gefunden". Fällt Helix oder der Token aus, liefert der Handler `reward_present: null` und das Dashboard zeigt "Status unbekannt" statt "fehlt".
