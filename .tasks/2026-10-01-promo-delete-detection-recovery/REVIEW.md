status: aktiv | Datum: 2026-10-01

# Gate Review R1

Quelle: `gate_hook.py --review --repo /home/nathanael/.worktrees/luna-dispatch-deadlock-twitch-bot-promo-delete-detection-20260915-364ad1c8 --base main --head codex/luna-dispatch/deadlock-twitch-bot/promo-delete-detection-20260915-364ad1c8`
Urteil: `gpt-6.1-sol BLOCK`, Exit 1.

1. **BLOCKING, Event-Reihenfolge:** `rust/crates/tb-chat/src/promos.rs:744` legt den Audit-Datensatz erst nach dem Senden an. Die drei Sendepfade liegen bei Zeile 2316, 2367 und 2990. Ein früheres Announcement-Event findet keinen Datensatz und wird verworfen. Lösung laut Gate: Announcement puffern oder die Lieferung vor dem Senden registrieren.
2. **BLOCKING, Regressionstest:** `rust/crates/tb-monitoring/tests/subscriptions.rs:1043` erwartet zwei Creates, obwohl `CHAT_SUB_TYPES` jetzt drei Subscription-Typen enthält.
3. **NIT, parallele Alerts:** `rust/crates/tb-chat/src/promos.rs:787` liest, sendet und markiert ohne Synchronisierung. Gleichzeitige Announcement- und Delete-Beobachter können doppelte Discord-Alerts senden.
4. **NIT, Zustellwiederholung:** `rust/crates/tb-chat/src/promos.rs:841` beendet bei fehlgeschlagener Discord-Zustellung ohne neuen Versuch. Der Dispatcher meldet das Event trotzdem als verarbeitet.
5. **NIT, OBS-Weiterleitung:** `rust/bin/tb-bot/src/obs_dock.rs:812` implementiert den Hook-Wrapper. Er delegiert `on_chat_message`, aber nicht `on_chat_message_delete` oder `on_chat_announcement_notification`; beide neuen Hooks fallen bei aktiviertem OBS-Dock auf die Default-No-ops zurück.

## Gate Review R2

Quelle: `gate_hook.py --review --repo /home/nathanael/.worktrees/luna-dispatch-deadlock-twitch-bot-promo-delete-detection-20260915-364ad1c8 --base main --head codex/luna-dispatch/deadlock-twitch-bot/promo-delete-detection-20260915-364ad1c8`
Urteil: `gpt-6.1-sol BLOCK`, Exit 1.

## Gate Review R3

Quelle: `gate_hook.py --review --repo /home/nathanael/.worktrees/luna-dispatch-deadlock-twitch-bot-promo-delete-detection-20260915-364ad1c8 --base main --head codex/luna-dispatch/deadlock-twitch-bot/promo-delete-detection-20260915-364ad1c8`
Urteil: `gpt-6.1-sol BLOCK`, Exit 1, geprüfter Head `a634e834328ea39c08163bcb4873d489d889a5ea`.

1. **BLOCKING, Alert-Wiederherstellung:** `promos.rs:1055` und `promos.rs:851`. Ein fehlgeschlagener Alert nach der Announcement-Korrelation bleibt ungesendet. Der Retry findet den gebundenen Audit-Datensatz nicht mehr, puffert das Event erneut und liefert Erfolg. Im Delivery-Pfad wird derselbe Alert-Fehler nur protokolliert.
2. **BLOCKING, Lösch-Race:** `promos.rs:1164`. Die Delete-Persistierung synchronisiert nicht mit beiden Korrelationstransaktionen. Ein paralleles Delete kann vor dem Commit der Message-ID-Zuordnung aktualisieren und dabei keinen Audit-Datensatz finden.

Fixes: ausstehende Alerts werden im vorhandenen 60-Sekunden-Promo-Loop erneut versucht; wiederholte Announcement-Events prüfen gebundene Audit-IDs; Announcement-Binder und Delete-Handler nehmen denselben Message-ID-Advisory-Lock vor dem Audit-Update.

## Gate Review R4

Quelle: `gate_hook.py --review --repo /home/nathanael/.worktrees/luna-dispatch-deadlock-twitch-bot-promo-delete-detection-20260915-364ad1c8 --base main --head codex/luna-dispatch/deadlock-twitch-bot/promo-delete-detection-20260915-364ad1c8`
Urteil: `gpt-6.1-sol ALLOW`, geprüfter Head `c2b2116150442c3340066ea6283cf5c427a20868`.

Begründung: Alert-Wiederherstellung und Synchronisierung der Löschkorrelation sind behoben; keine nachgewiesene Regression im Fix-Diff.

## Gate Review R5

Quelle: `gate_hook.py --review --repo /home/nathanael/.worktrees/luna-dispatch-deadlock-twitch-bot-promo-delete-detection-20260915-364ad1c8 --base main --head codex/luna-dispatch/deadlock-twitch-bot/promo-delete-detection-20260915-364ad1c8`
Urteil: `gpt-6.1-sol BLOCK`, Exit 1, geprüft nach Commit `dd7e9e64`.

1. **BLOCKING, nicht persistierte Zustellung:** `promos.rs:758`. `record_promo_delivery` protokolliert DB-Fehler und verwirft die angenommene Lieferung. Bei transientem DB-Fehler bleibt kein Audit-Datensatz; das gepufferte Announcement kann ihn nicht wiederherstellen. Betroffen sind alle drei Aufrufer: `reason`, `timeout_pitch` und `lurker_tax`.
2. **BLOCKING, doppelte Announcement-ID:** `observe_announcement_notification`. Der Handler bindet erst einen offenen Delivery-Datensatz und prüft danach die vorhandene Message-ID. Bei einem zweiten passenden offenen Datensatz kann die Wiederholung am Unique-Index scheitern, bevor sie den bestehenden Alert-Retry erreicht.

Die unabhängige Intent-Abnahme von Commit `c2b21161` erfolgte vor diesem Gate-Befund und ist für die aktuelle Runde nicht abschließend.
