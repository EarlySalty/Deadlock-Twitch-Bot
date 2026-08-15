# Bot Token Scopes

Stand: 2026-08-15

Dieses Dokument beschreibt den aktuell gewollten Scope-Satz fuer den zentralen Bot-Account, wie der Token neu erzeugt wird und wo er im laufenden Setup liegt.

Die verbindliche Liste im Code ist `REQUIRED_BOT_SCOPES` in `rust/bin/tb-bot/src/chat_wiring.rs`. Der Bot prueft sie beim Boot gegen den validierten Token und warnt pro fehlendem Scope. Aendert sich diese Liste, muss der CLI-Befehl weiter unten mitgezogen werden.

## Nachtrag 2026-08-15

Der Scope-Satz wurde um die Pfade erweitert, die der Rust-Bot inzwischen wirklich faehrt:

- `user:bot` und `user:manage:whispers` waren im Boot-Check schon Pflicht, standen aber nicht im CLI-Befehl.
- `user:read:whispers` kommt neu dazu, damit der Bot Antworten von Streamern auf seine Whispers lesen kann.
- `moderator:manage:banned_users`, `moderator:manage:shoutouts` und `moderator:read:followers` sind fuer die Moderator-Telemetrie-EventSubs (`tb_monitoring::MODERATOR_TELEMETRY_SUBSCRIPTIONS`) verbindlich, nicht mehr nur nice to have. Ohne sie ueberspringt der Reconcile den Bot-Pfad und faellt auf den Broadcaster-Token zurueck.

Diese Scopes wirken erst nach einer neuen Autorisierung des Bot-Accounts (siehe "Re-Auth ausloesen").

## Ergebnis vom 17.03.2026

- Bot-Login: `deutschedeadlockcommunity`
- Token neu erzeugt via Twitch CLI
- Danach in Windows Credential Manager unter Service `DeadlockBot` gespeichert:
  - `TWITCH_BOT_TOKEN`
  - `TWITCH_BOT_REFRESH_TOKEN`
- Validiert gegen `https://id.twitch.tv/oauth2/validate`
- Ergebnis der Validierung:
  - `scope_count = 30`
  - `moderator:read:chatters` vorhanden
  - `moderator:manage:announcements` vorhanden
  - `user:read:chat` vorhanden
  - `user:write:chat` vorhanden

## Gewaehlte Strategie

Der Bot bekommt absichtlich einen grosszuegigen Moderator-Scope-Satz:

- alle offiziellen `moderator:*`-Scopes, die Twitch am 2026-03-17 dokumentiert
- plus die zentralen Chat- und Whisper-Scopes:
  - `user:bot`
  - `user:read:chat`
  - `user:write:chat`
  - `user:manage:whispers`
  - `user:read:whispers`

Hintergrund:

- Der akute Produktionsfehler war `moderator:read:chatters`.
- Der Bot nutzt bereits mehrere Moderator-Endpunkte und Moderator-EventSub-Pfade.
- Fuer den Bot-Token ist ein leichter Scope-Ueberschuss aktuell gewollt, damit bei neuen Moderator-Features nicht sofort wieder eine Re-Auth noetig wird.

## Voller Scope-Satz

### Chat und Whisper

- `user:bot`
- `user:read:chat`
- `user:write:chat`
- `user:manage:whispers`
- `user:read:whispers`

### Moderator Read

- `moderator:read:automod_settings`
- `moderator:read:banned_users`
- `moderator:read:blocked_terms`
- `moderator:read:chat_messages`
- `moderator:read:chat_settings`
- `moderator:read:chatters`
- `moderator:read:followers`
- `moderator:read:guest_star`
- `moderator:read:moderators`
- `moderator:read:shield_mode`
- `moderator:read:shoutouts`
- `moderator:read:suspicious_users`
- `moderator:read:unban_requests`
- `moderator:read:vips`
- `moderator:read:warnings`

### Moderator Manage

- `moderator:manage:announcements`
- `moderator:manage:automod`
- `moderator:manage:automod_settings`
- `moderator:manage:banned_users`
- `moderator:manage:blocked_terms`
- `moderator:manage:chat_messages`
- `moderator:manage:chat_settings`
- `moderator:manage:guest_star`
- `moderator:manage:shield_mode`
- `moderator:manage:shoutouts`
- `moderator:manage:suspicious_users`
- `moderator:manage:unban_requests`
- `moderator:manage:warnings`

## Code-pfade, die heute direkt davon profitieren

- Boot-Pruefung des Bot-Tokens:
  - `rust/bin/tb-bot/src/chat_wiring.rs` (`REQUIRED_BOT_SCOPES`)
- Chat lesen und senden, Whisper senden:
  - `rust/crates/tb-chat/src/moderation.rs`, `rust/crates/tb-chat/src/api.rs`, `rust/crates/tb-transport-twitch/src/chat.rs`
  - braucht `user:bot`, `user:read:chat`, `user:write:chat`, `user:manage:whispers`
- Antworten von Streamern auf Bot-Whispers lesen:
  - braucht `user:read:whispers` (Twitch-EventSub `user.whisper.message`)
  - der Empfangspfad wird nachgezogen, der Scope muss vorher im Token stehen
- Moderator-Telemetrie-EventSub ueber den Bot-Token:
  - `rust/crates/tb-monitoring/src/subscriptions.rs` (`MODERATOR_TELEMETRY_SUBSCRIPTIONS`)
  - braucht:
    - `moderator:manage:banned_users` (`channel.ban`, `channel.unban`)
    - `moderator:manage:shoutouts` (`channel.shoutout.create`, `channel.shoutout.receive`)
    - `moderator:read:followers` (`channel.follow` v2)
- Follower-Total ueber Bot-Token:
  - `rust/crates/tb-raid/src/bot_oauth.rs` (`can_read_followers`), `rust/bin/tb-bot/src/wiring.rs`
  - braucht `moderator:read:followers`
- Chatters-Quelle ueber Bot-Token:
  - `rust/crates/tb-chat/src/promos.rs` (`has_chatters_scope`), `rust/bin/tb-bot/src/chatters_wiring.rs`
  - braucht `moderator:read:chatters`
- Dashboard-Announcements:
  - `rust/crates/tb-chat/src/moderation.rs`
  - braucht `moderator:manage:announcements`

Nicht zu verwechseln mit den Broadcaster-Scopes der Streamer
(`rust/crates/tb-analytics/src/system_oauth_scopes.rs`, `REQUIRED_SCOPES`) und
mit dem separaten Engagement-Sende-Account
(`rust/crates/tb-engagement/src/sender_auth.rs`, eigener Token, eigener
Authorize-Flow). Das sind drei getrennte Tokens.

## Bewusst nicht im neuen Bot-Token enthalten

- `user:read:follows`
  - wird nur fuer den Best-Effort-Follow-Check verwendet
  - kein Produktionsblocker
- Broadcaster-Scopes wie `channel:manage:raids`, `channel:read:subscriptions`, `channel:read:ads`, `channel:read:redemptions`, `bits:read`, `channel:read:hype_train`, `clips:edit`
  - gehoeren fachlich nicht zum zentralen Moderator-Bot-Token
  - laufen streamer-seitig oder sind fuer den Bot derzeit nicht kritisch

## Re-Auth ausloesen

Ein Refresh aendert die Scopes nie. Neue Scopes kommen nur ueber eine frische
Autorisierung des Bot-Accounts `deutschedeadlockcommunity`. Es gibt dafuer
bewusst keinen Endpunkt im Bot: der Bot liest seinen Token nur aus der Env
(`TWITCH_BOT_TOKEN`, `TWITCH_BOT_REFRESH_TOKEN`, gefuellt aus Infisical) und
schreibt Rotationen best effort dorthin zurueck (ADR 0005). Die Erst-Autorisierung
laeuft manuell.

1. Im Browser als `deutschedeadlockcommunity` bei Twitch angemeldet sein, dann
   die Twitch CLI mit Client-ID und Secret der Bot-App verbinden
   (`twitch configure`, Redirect `http://localhost:3000` muss in der App
   eingetragen sein).
2. Token mit dem vollen Scope-Satz ziehen:

```bash
twitch token -u -s "moderator:manage:announcements moderator:manage:automod moderator:read:automod_settings moderator:manage:automod_settings moderator:read:banned_users moderator:manage:banned_users moderator:read:blocked_terms moderator:manage:blocked_terms moderator:read:chat_messages moderator:manage:chat_messages moderator:read:chat_settings moderator:manage:chat_settings moderator:read:chatters moderator:read:followers moderator:read:guest_star moderator:manage:guest_star moderator:read:moderators moderator:read:shield_mode moderator:manage:shield_mode moderator:read:shoutouts moderator:manage:shoutouts moderator:read:suspicious_users moderator:manage:suspicious_users moderator:read:unban_requests moderator:manage:unban_requests moderator:read:vips moderator:read:warnings moderator:manage:warnings user:bot user:read:chat user:write:chat user:manage:whispers user:read:whispers"
```

3. Ergebnis gegen `https://id.twitch.tv/oauth2/validate` pruefen und die
   Scope-Liste mit `REQUIRED_BOT_SCOPES` abgleichen.
4. Access- und Refresh-Token in Infisical (`http://127.0.0.1:8080`) unter
   `TWITCH_BOT_TOKEN` und `TWITCH_BOT_REFRESH_TOKEN` setzen. Nicht in Dateien
   loggen und nicht in die Shell-History schreiben.
5. Dienst neu starten, damit das alte Token-Paar aus dem RAM faellt:

```bash
XDG_RUNTIME_DIR=/run/user/1000 systemctl --user restart deadlock-twitch-bot-rust.service
```

6. Im Log pruefen, dass beim Boot keine Zeile `Bot-Token ohne Scope ...` mehr
   auftaucht.

Der Dienst liest die Secrets beim Start ueber `rust/scripts/run_tb_bot_service.sh`
und `dl-infisical-env --profile all`.

## Offizielle Referenzen

- Twitch Scopes: `https://dev.twitch.tv/docs/authentication/scopes/`
- Twitch CLI Token Command: `https://dev.twitch.tv/docs/cli/token-command/`
