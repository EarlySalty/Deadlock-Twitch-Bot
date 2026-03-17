# Bot Token Scopes

Stand: 2026-03-17

Dieses Dokument beschreibt den aktuell gewollten Scope-Satz fuer den zentralen Bot-Account, wie der Token neu erzeugt wurde und wo er im lokalen Setup liegt.

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
- plus die zentralen Chat-Scopes:
  - `user:read:chat`
  - `user:write:chat`

Hintergrund:

- Der akute Produktionsfehler war `moderator:read:chatters`.
- Der Bot nutzt bereits mehrere Moderator-Endpunkte und Moderator-EventSub-Pfade.
- Fuer den Bot-Token ist ein leichter Scope-Ueberschuss aktuell gewollt, damit bei neuen Moderator-Features nicht sofort wieder eine Re-Auth noetig wird.

## Voller Scope-Satz

### Chat

- `user:read:chat`
- `user:write:chat`

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

- Chatters-Fallback ueber Bot-Token:
  - `bot/analytics/mixin.py`
  - braucht `moderator:read:chatters`
- Follower-Fallback ueber Bot-Token:
  - `bot/monitoring/sessions_mixin.py`
  - `bot/raid/bot.py`
  - braucht `moderator:read:followers`
- Moderator-EventSub ueber Bot-Token:
  - `bot/monitoring/eventsub_mixin.py`
  - braucht unter anderem:
    - `moderator:manage:banned_users`
    - `moderator:manage:shoutouts`
    - `moderator:read:followers`
- Chat senden:
  - `bot/chat/moderation.py`
  - braucht `user:write:chat`
- Chat lesen / Chat-EventSub:
  - `bot/chat/connection.py`
  - braucht `user:read:chat`
- Dashboard-Announcements:
  - `bot/chat/moderation.py`
  - braucht `moderator:manage:announcements`

## Bewusst nicht im neuen Bot-Token enthalten

- `user:bot`
  - bleibt optional
  - wird erst noetig, wenn der Chat-Flow auf App-Token-EventSub umgebaut wird
- `user:read:follows`
  - wird nur fuer den Best-Effort-Follow-Check verwendet
  - kein Produktionsblocker
- Broadcaster-Scopes wie `channel:manage:raids`, `channel:read:subscriptions`, `channel:read:ads`, `channel:read:redemptions`, `bits:read`, `channel:read:hype_train`, `clips:edit`
  - gehoeren fachlich nicht zum zentralen Moderator-Bot-Token
  - laufen streamer-seitig oder sind fuer den Bot derzeit nicht kritisch

## Reproduzierbarer CLI-Befehl

```powershell
twitch token -u -s "moderator:manage:announcements moderator:manage:automod moderator:read:automod_settings moderator:manage:automod_settings moderator:read:banned_users moderator:manage:banned_users moderator:read:blocked_terms moderator:manage:blocked_terms moderator:read:chat_messages moderator:manage:chat_messages moderator:read:chat_settings moderator:manage:chat_settings moderator:read:chatters moderator:read:followers moderator:read:guest_star moderator:manage:guest_star moderator:read:moderators moderator:read:shield_mode moderator:manage:shield_mode moderator:read:shoutouts moderator:manage:shoutouts moderator:read:suspicious_users moderator:manage:suspicious_users moderator:read:unban_requests moderator:manage:unban_requests moderator:read:vips moderator:read:warnings moderator:manage:warnings user:read:chat user:write:chat"
```

Danach die Rueckgabe nicht in Dateien loggen, sondern direkt in den Credential Store schreiben.

## Lokale Speicherung

Der Runtime-Code liest Secrets aus `DeadlockBot` in Windows Credential Manager:

- `TWITCH_BOT_TOKEN`
- `TWITCH_BOT_REFRESH_TOKEN`
- `TWITCH_BOT_CLIENT_ID`
- optional `TWITCH_BOT_CLIENT_SECRET`

Relevante Loader:

- `bot/secret_store.py`
- `bot/api/token_manager.py`
- `bot/chat/tokens.py`

## Nach der Re-Auth

Wenn die Twitch-Runtime bereits laeuft, sollte sie nach dem Schreiben des neuen
Access-/Refresh-Tokens neu gestartet werden. Sonst haelt der Prozess unter
Umstaenden noch das alte Token-Paar im RAM.

```powershell
Restart-Service -Name "Deadlock-twitch-bot-service","Deadlock-twitch-dashboard-service" -Force
```

## Offizielle Referenzen

- Twitch Scopes: `https://dev.twitch.tv/docs/authentication/scopes/`
- Twitch CLI Token Command: `https://dev.twitch.tv/docs/cli/token-command/`
