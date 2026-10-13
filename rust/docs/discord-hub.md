# Discord Hub

The Hub (`tb-discord-hub`) is the Twitch bot's Discord integration for foreign servers.

## Database

Set `DATABASE_URL` to the Twitch Postgres DSN. Guild events then persist:

- `GUILD_CREATE` upserts `discord_guilds` (`status = active`)
- `GUILD_DELETE` marks `status = removed`
- `GUILD_MEMBER_REMOVE` deactivates matching `streamer_guilds` via `twitch_streamer_identities.discord_user_id`

Without `DATABASE_URL` the Hub still forwards to the broker and logs gateway events (`LoggingSink`). READY replay stays idempotent.

## `/setup`

Global slash command. English text on foreign servers. Options: `language`, `announce_channel`, `alert_channel`, `streamer_role`, `ping_role`, `invite`, `auto_partner`. The ping role must be mentionable and sit below the Hub role. On the German community guild the command only shows the existing notice and does not write.

## Onboarding Discord OAuth

Streamer onboarding uses the Hub Discord app (`DISCORD_HUB_CLIENT_ID` / `DISCORD_HUB_CLIENT_SECRET`) with scopes `identify guilds`. Redirect URI: `TB_ONBOARDING_DISCORD_REDIRECT_URI`. Access tokens are never stored.

## systemd und Aktivierung

Die Release-Skripte verlangen `rust/target/release/tb-discord-hub`, sobald der angeforderte Commit das Hub-Paket enthält. Der Installer prüft den eingebetteten `.twitch_build`-SHA und archiviert Launcher und Unit aus diesem Commit. Der verantwortliche Release-Build muss deshalb auch `tb-discord-hub` bauen.

Der Deploy-Wrapper installiert `deadlock-twitch-discord-hub.service`, aktiviert oder startet sie aber nicht automatisch. Standard-Restarts und `--restart all` bleiben auf Dashboard, Bot und Coaching-Watch begrenzt. `--restart discord-hub` setzt eine bereits installierte Unit sowie die Betreiberkonfiguration voraus.

### Voraussetzungen des Betreibers

1. Eine eigene Discord-Anwendung bereitstellen. In Infisical `DISCORD_HUB_BOT_TOKEN`, `DISCORD_HUB_CLIENT_ID` und `DISCORD_HUB_CLIENT_SECRET` hinterlegen. Der Launcher verwendet `DISCORD_HUB_APPLICATION_ID`, falls gesetzt, sonst `DISCORD_HUB_CLIENT_ID`; beide IDs müssen übereinstimmen. Das Client-Secret wird vom OAuth-Dashboard gebraucht und nicht an den Hub weitergegeben. Members-Intent aktivieren. Einladungsrechte: Kanäle sehen, Nachrichten senden, Links einbetten und Rollen verwalten; weder Administrator noch Mention Everyone. Ping-Rollen müssen erwähnbar sein und unterhalb der Hub-Rolle stehen.
2. Den vorhandenen Infisical-Loader `/usr/local/libexec/dl-infisical-env` und das bestehende verschlüsselte Infrastruktur-Credential `/etc/credstore.encrypted/deadlock-twitch-infisical.cred` wiederverwenden. Keine neuen Token- oder Environment-Dateien anlegen. Der Dienst lädt Secrets zur Laufzeit und entfernt fremde Secretvariablen vor dem Binary-Start. Broker-Token-Kette: `MASTER_BROKER_TOKEN`, `MAIN_BOT_INTERNAL_TOKEN`, `TWITCH_INTERNAL_API_TOKEN`. `DISCORD_HUB_INTERNAL_TOKEN` ist optional, sonst gilt der Broker-Token.
3. `DISCORD_HUB_DATABASE_URL` in Infisical auf eine eingeschränkte Postgres-Rolle für Hub-Guild- und Mitgliedschaftsdaten setzen. Die Unit läuft als dynamischer Benutzer `twitchhub`, nicht als root oder `twitchbot`. Kein DB-Administrator und keine ungeprüfte Übernahme der Bot-DSN. Migrationen und Tabellenrechte separat durch den Integrator prüfen. Der Launcher übergibt diese DSN als `DATABASE_URL`; fehlt sie, bricht er ab statt in den Logging-only-Modus zu wechseln.

Die root-verwaltete Datei `/etc/deadlock-twitch/discord-hub.conf` enthält diese nicht geheimen Schlüssel als unquotierte `NAME=WERT`-Zeilen ohne Shell-Befehle oder Kommentare:

- `INFISICAL_PROJECT_ID` und `INFISICAL_ENV`: vorhandene Projektmetadaten übernehmen.
- `INFISICAL_SECRET_PATH`: optionaler Infisical-Pfad, Standard `/`.
- `DISCORD_HUB_ENABLED=1`: bewusste Aktivierung, ohne diesen Wert bleibt der Launcher gesperrt.
- `DISCORD_HUB_PORT=8779`: Dienst-Port. `127.0.0.1:8779` wurde am 13.10.2026 durch eine Bind-Probe als frei bestätigt; vor Aktivierung erneut prüfen. Der Rust-Standalone-Standard bleibt `8771`, der hier bereits von dl-web belegt ist. Der Service-Launcher setzt ausdrücklich `8779` und erzwingt `127.0.0.1`.

Die Datei muss für den dynamischen Dienstbenutzer lesbar sein, aber ausschließlich von root schreibbar, etwa `root:root` mit Modus `0644`. Die Unit verwendet kein `EnvironmentFile`. Solange die Datei fehlt, überspringt systemd den Dienst; ein expliziter Hub-Deploy-Restart meldet fehlende Aktivierung als Fehler.

### Abnahme und Umschaltung

Nach App-, DB- und Betreiberkonfiguration startet der Integrator einen geprüften Release mit `deploy-twitch-release <SHA> <Release-Clone> --restart discord-hub`. Der Wrapper prüft danach die laufende Hub-exe gegen `current`. `deploy-twitch-release --pruefen --hub` liefert denselben ausschließlich lesenden Hub-Nachweis; ohne `--hub` werden weiter die deutschen Twitch-Dienste geprüft.

PID-Wechsel, Journalfehler, `GET http://127.0.0.1:8779/health`, Gateway-Bereitschaft und die tatsächliche Wirkung zusätzlich prüfen. Health allein beweist keine Discord-Anmeldung. Erst nach belegter Hub-Bereitschaft darf der Integrator `DISCORD_HUB_BASE_URL=http://127.0.0.1:8779` für die vorgesehenen Twitch-Dienste konfigurieren und sie gezielt neu starten. Bis dahin bleiben Broker-Adresse und deutscher Betrieb unverändert. Dauerhafte systemd-Aktivierung ist ein separater Betreiber-Schritt.

Ohne bereitgestellte Discord-Anwendung und App-Credentials sind Gateway-Anmeldung, OAuth, Testserver-Installation und Umschaltung nicht überprüfbar. Quellunterstützung bedeutet nicht, dass der Hub live ist.
