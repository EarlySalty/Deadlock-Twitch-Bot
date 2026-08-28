# EVIDENCE: Sofort-Reconcile feuert auf Re-Auth-Pfad nicht

Basis: Branch `fix/chat-abo-reauth` von `origin/main` (41728b99).

## Belegte Wurzel
Der Sofort-Trigger `request_chat_subscription_reconcile` wird in `oauth_callback` NUR im
Erst-Auth-Arm aufgerufen. Alle Re-Auth-Arme (`had_existing_auth == true`) rufen ihn nie auf.

- `rust/bin/tb-bot/src/raid_oauth_impl.rs:1469` : `match (&self.partner_setup, had_existing_auth)`.
- `rust/bin/tb-bot/src/raid_oauth_impl.rs:1470` : Arm `(Some(setup), false)` = Erst-Auth.
- `rust/bin/tb-bot/src/raid_oauth_impl.rs:1486` : hier (und NUR hier) `request_chat_subscription_reconcile(...)`.
- `rust/bin/tb-bot/src/raid_oauth_impl.rs:1496` : Arm `(Some(setup), true) if sync_existing_auth` = Re-Auth, KEIN Trigger.
- `rust/bin/tb-bot/src/raid_oauth_impl.rs:1513` : Arm `(Some(_), true)` = Re-Auth, leer, KEIN Trigger.
- `rust/bin/tb-bot/src/raid_oauth_impl.rs:1521` : Arm `(None, true)` = Re-Auth ohne PartnerSetup, leer, KEIN Trigger.

Folge: deusasta hatte bereits eine Auth-Zeile, also `had_existing_auth == true`. Der Persist
(`store_new_auth`) setzte needs_reauth=FALSE und channel:bot korrekt, aber der Sofort-Trigger
wurde nie gefeuert. Das `channel.chat.message`-Abo entstand erst im naechsten 30-Min-Takt.

## Persist ist immer vor dem Followup-Match und immer committed
- `rust/bin/tb-bot/src/raid_oauth_impl.rs:1412` : `had_existing_auth = has_saved_auth_record(...)`.
- `rust/bin/tb-bot/src/raid_oauth_impl.rs:1427` : `store_new_auth(...)` (commit), Fehler => early return.
- `rust/crates/tb-raid/src/auth_writer.rs:170` : `UPDATE ... SET needs_reauth = FALSE ...` im selben Tx.
Der Grant ist nach dem Persist committed und per SELECT sofort sichtbar; ein Trigger direkt
danach ist race-frei.

## Trigger- und Reconcile-Mechanik (funktioniert, nur falsch platziert)
- `rust/bin/tb-bot/src/raid_oauth_impl.rs:1705` : `request_chat_subscription_reconcile` ruft bei Ok `notify.notify_one()`.
- `rust/bin/tb-bot/src/main.rs:864` : eine gemeinsame `Notify`-Instanz.
- `rust/bin/tb-bot/src/main.rs:947` : geht als `reconcile_now` in den Reconcile-Loop.
- `rust/bin/tb-bot/src/main.rs:1331` : dieselbe Instanz in den Raid-OAuth-Adapter.
- `rust/bin/tb-bot/src/chat_wiring.rs:990-999` : `select! { tick / reconcile_now.notified() }` -> `reconcile_chat_subscriptions`.
- `rust/bin/tb-bot/src/chat_wiring.rs:74` : `CHAT_SUB_RECONCILE_INTERVAL = 30 min` (Fallback-Takt).
- `rust/bin/tb-bot/src/chat_wiring.rs:1036-1080` : SQL-Filter `needs_reauth = FALSE AND scopes LIKE '%channel:bot%'` (korrekt).

## Roter Regressionstest (vor dem Fix)
`reauth_triggert_sofortigen_chat_subscription_reconcile` in `mod db_tests`:
bestehende Auth-Zeile (Re-Auth), PartnerSetup=None => Arm `(None, true)`. Erwartet, dass der
Notify sofort feuert. Vor dem Fix: Timeout (kein Trigger) => Test rot.
