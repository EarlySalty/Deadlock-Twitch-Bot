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

## Offene Punkte (bewusst nicht in diesem Paket)

- REQ-5 Laufzeit-Gate ist im Code (`LurkerRewardChecker`, `PromoEngine::set_lurker_reward_checker`) und getestet, aber der produktive Adapter ist in `bin/tb-bot` noch nicht verdrahtet. Grund: `build_runtime` bekommt den Streamer-`TokenProvider` (Scope `channel:read:redemptions`) nicht durchgereicht; die Verdrahtung braucht eine Signaturerweiterung von `build_runtime` plus `main.rs`. Ohne Adapter bleibt `reward_checker = None`, die Erinnerung verhält sich wie bisher (Plan- und Scope-Gate greifen, kein Fehlverhalten). Der Transport (`HelixClient::get_custom_rewards`) ist fertig.
- REQ-6 `reward_present` im Dashboard kommt aus `twitch_channel_points_events` (eine passende Einlösung beweist die Belohnung). Eine frisch angelegte, noch nie eingelöste Belohnung zeigt daher "Belohnung fehlt", bis die erste Einlösung eintrifft. Die autoritative Helix-Prüfung im GET-Handler ist blockiert, weil die committeten Tests die Handler-Signatur `(auth, State<PgPool>, Query)` festschreiben; ein Helix-Zugriff bräuchte eine State-Erweiterung der dashboard-api.
