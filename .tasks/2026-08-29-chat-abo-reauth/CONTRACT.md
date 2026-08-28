# CONTRACT: Chat-Abo nach Re-Auth sofort anlegen

Status: eingefroren nach Anlegen. Korrekturen nur als `## Amendments`.

## Ziel
Nach JEDER erfolgreichen (Re-)Autorisierung eines Kanals wird das Chat-Subscription-Reconcile
fuer genau diesen Kanal sofort angestossen, sodass das `channel.chat.message`-Abo unmittelbar
neu angelegt wird und der Kanal nicht bis zu 30 Minuten (naechster Reconcile-Takt) tot bleibt.

## REQ
- REQ-1: Ein erfolgreicher `oauth_callback` (Persist via `AuthWriter::store_new_auth` ok) loest den
  Chat-Subscription-Reconcile-Trigger (`chat_subscription_reconcile` Notify) sofort aus.
- REQ-2: Das gilt fuer den Erst-Auth-Pfad (`had_existing_auth == false`) UND fuer JEDEN
  Re-Auth-Pfad (`had_existing_auth == true`), unabhaengig davon, ob ein `PartnerSetupService`
  verdrahtet ist und ob ein Followup laeuft.
- REQ-3: Der Trigger feuert erst NACH erfolgreichem Persist (needs_reauth=FALSE, aktualisierte
  Scopes committed), nie bei fehlgeschlagenem Persist (ScopeMismatch, EncryptionFailed, DB-Fehler).

## INV
- INV-1: Kein neues LLM-Modell, keine ENV-Datei, keine neuen Secrets. Secrets aus Infisical.
- INV-2: Bestehendes Reconcile-Intervall (30 Min, `CHAT_SUB_RECONCILE_INTERVAL`) bleibt als
  Fallback unveraendert; der Fix ergaenzt nur den Sofort-Trigger.
- INV-3: Der Persist-Pfad selbst (Scope-Pruefung, Verschluesselung, needs_reauth-Reset) bleibt
  unveraendert.
- INV-4: Der bestehende Regressionstest `reconcile_signal_follows_partner_setup_result` bleibt gruen.

## Nicht-Ziele
- Keine Aenderung an der Dashboard-/Uplink-OAuth-Redirect-Logik.
- Keine Aenderung an `select_chat_subscription_channels`/`reconcile_chat_subscriptions` (die
  SQL-Auswahl ist korrekt; das Problem ist allein der fehlende Sofort-Trigger auf dem Re-Auth-Pfad).
- Keine Aenderung am 30-Min-Fallback-Takt.

## Erlaubter Bereich
- `rust/bin/tb-bot/src/raid_oauth_impl.rs` (Trigger-Aufruf in `oauth_callback` plus Regressionstest).
- `.tasks/2026-08-29-chat-abo-reauth/` (Artefakte).
