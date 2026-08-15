# Bot-Ban-Erkennung

Stand 2026-08-16. Branch `fix/ban-klassifikation`.

## Gebaut

Ban-Klassifikation neu hergeleitet. `c246502a` bleibt nicht stehen:
`POST /moderation/moderators` plus `400 {"message":"user is banned"}` ist kein Bann.

Reihenfolge:

1. `sender_banned` beim Chat-Senden. Einziger harter Beweis. Setzt `bot_banned`.
2. Chat-Lesbarkeit als Gegenprobe. Sieht der Bot Chatter, ist er nicht gebannt.
   Der Dienst nimmt eine eigene `technical_pause_reason='bot_banned'` dann selbst zurück.
3. `GET /moderation/banned` bleibt draußen (Scope `moderation:read` fehlt).
4. Offline-Sendeprobe ist vorbereitet (`TB_BOT_BAN_SEND_PROBE=1`), default aus.
   Schickt nie, solange der Kanal live ist.

Solange der Beweis nicht hart ist: nur Admin-Log, niemand wird pausiert.
Meldung nur beim Zustandswechsel, einmal pro Vorfall
(`twitch_bot_ban_klassifikation`).

Bot-Bann und ungültiger Token bleiben getrennte Zustände. 401 ist kein Bann.
EventSub-403 plus Moderator-400 ruft den Lifecycle nicht mehr.

Tests: Moderator-400 plus sichtbare Chatter ohne Pause.
`sender_banned` setzt den Zustand. Vorher `bot_banned`, danach Chatter sichtbar,
Pause weg. 340 `tb-raid` lib, 15 `timeout_tracking`, EventSub-Pfade grün.

## Bewusst offen

- **Deploy.** Nicht in dieser Session. Genau ein Deployer, mit der
  Parallel-Session abstimmen. Nach Deploy würde der Dienst selbst entpausen:
  `whysolowkey` (Chatter am 2026-08-15) und `duzzel` (Chat und Chatter am
  2026-08-15). `pixelpiratemarvin` bleibt pausiert, dort gibt es seit Juli
  keine Chatter mehr.
- **Disconnect `46haris` und `talakos86`.** Über `disconnect_bot`, nicht hier.
  Beide hatten nie einen Deadlock-Stream.
- **Marker-Review `pixelpiratemarvin`.** EventSub-Pfad, kein aktueller
  Gegenbeweis. Nicht per Hand in der DB biegen.

## Nicht anfassen

Parallel-Worktree `tb-ban-entprellen` / Branch `fix/ban-verdacht-entprellen`.
Anderes Thema.
