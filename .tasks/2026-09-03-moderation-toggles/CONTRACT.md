# Contract: Moderations-Toggles im Verwaltungsdashboard

## Ziel
Der Streamer kann im Twitch-Verwaltungsdashboard (dashboard_v2) je Moderationsfunktion des Bots einzeln an- und ausschalten, welche Aufgaben der Bot in seinem Kanal übernimmt.

## Umfang (erlaubter Bereich)
- `rust/crates/tb-chat/src/pipeline.rs`
- `rust/crates/tb-chat/src/moderation_settings.rs` (neu)
- `rust/crates/tb-chat/src/lib.rs`
- `rust/crates/tb-dashboard-api/src/handlers/moderation_settings.rs` (neu)
- `rust/crates/tb-dashboard-api/src/handlers/mod.rs`
- `rust/crates/tb-dashboard-api/src/lib.rs`
- `dl-central-db/migrations/` (neue Migration)
- `bot/dashboard_v2/src/pages/Verwaltung.tsx`
- `bot/dashboard_v2/src/components/verwaltung/ModerationSection.tsx` (neu)
- `bot/dashboard_v2/src/api/moderation.ts` (neu)

## REQ
- REQ1: Neue Tabelle `twitch_moderation_settings` mit Key `channel_user_id` (analog `twitch_scam_guard_settings`) und je Funktion einem `*_enabled`-BOOLEAN, Default TRUE: `global_ban_enabled`, `scam_pitch_enabled`, `spam_autoban_enabled`, `sus_invite_enabled`.
- REQ2: `ChatPipeline` lädt pro Kanal eine `ModerationSettings`-Struct gecacht (TTL wie Scam-Guard-Loader), und die Pipeline-Schritte Global-Ban (Schritt 5), Scam-Pitch (Schritt 6), Spam (Schritt 7), Sus-Invite (Schritt 8) prüfen ihr jeweiliges Flag und überspringen die Aktion sauber, wenn es false ist.
- REQ3: Backend GET+POST `/twitch/api/v2/streamer/moderation/settings` nach Vorbild `scam_guard_settings.rs`, Auth über `DashboardAuthLevel::Partner` mit `resolve_login`, Admin darf per `?streamer=` fremde Kanäle setzen, Upsert in die neue Tabelle.
- REQ4: Frontend `ModerationSection.tsx` mit einem Toggle je Funktion (Klartext-Beschriftung in Nutzersprache, keine internen Begriffe), API-Client `moderation.ts`, eingehängt in `Verwaltung.tsx`; Stil nach `ScamGuardSection.tsx`.

## INV
- INV1: Ist für einen Kanal keine Zeile gesetzt, verhält sich der Bot exakt wie heute (alle vier Funktionen aktiv). Default TRUE auf DB- und Loader-Ebene.
- INV2: Non-Partner-Kanäle bleiben unverändert (nur Tracking, keine Moderation).
- INV3: Der Conversation-Scam-Guard (eigene Tabelle/Sektion) und der Crew-Guard bleiben unangetastet.
- INV4: Keine zusätzliche DB-Last pro Nachricht: Settings müssen gecacht werden, nicht pro Event frisch geladen.

## Nicht-Ziele
- Kein Umbau oder Merge des bestehenden Scam-Guard-Toggles.
- Kein per-Kanal-Schalter für den Crew-Guard (bleibt globaler Env-Schalter).
- Keine granulare Aufspaltung einzelner Funktionen in Delete-vs-Ban-vs-Timeout; nur Funktion an/aus.
- Keine neuen LLM-Modelle, keine Änderung der Erkennungslogik selbst.

## Regressionstest (Pflicht, vor dem Fix rot)
In `tb-chat`: Test, der bei gesetztem `spam_autoban_enabled = false` (bzw. je Funktion) beweist, dass die Pipeline die Ban/Timeout/Delete-Aktion NICHT auslöst, und bei true (oder fehlender Zeile) weiterhin auslöst. Roter Lauf mit Testname und Fehlermeldung festhalten, bevor die Gates gebaut werden.
