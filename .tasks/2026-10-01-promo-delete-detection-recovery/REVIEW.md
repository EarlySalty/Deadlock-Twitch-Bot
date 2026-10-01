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

Der Gate vergleicht Git-Refs. Der angegebene Branch-Ref zeigte noch auf `364ad1c8d92c6701163b69d7608675ad08e50270`, daher enthielt dieser Lauf die uncommitteten Änderungen nicht. Gemeldet wurden erneut `promos.rs:744` und `subscriptions.rs:1043`. Kein Urteil zum aktuellen Arbeitsdiff. Die Orchestrator-Anweisung vom 2026-10-01 autorisiert nun, den WIP auf dem eigenen Branch zu sichern und den aktuellen Stand unabhängig abzunehmen. Der nächste Gate-Lauf muss gegen den neuen Commit erfolgen. `git diff --check` war sauber; keine Tests oder Builds liefen.
