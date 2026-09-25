# Verbindliche Aufteilung R2 nach Unterbrechung am 18.09., 10:00 UTC

Der Orchestrator hat ausschließlich Thread A mit thread.turn.interrupt sauber unterbrochen, um dessen gelieferte Transport-Grundlage unabhängig zu korrigieren. Bestehende Dateien und Fortschritt bleiben erhalten. Ab jetzt EINE DATEI, EIN BESITZER:

## A: tb-category-collector, Storage, Migration, systemd
Thread50f38cec setzt vorhandenen begonnenen Collector direkt fort (kein Neubeginn). Du besitzt ausschließlich:
- rust/bin/tb-category-collector/** inklusive Konfiguration, Spracherkennung, Discovery, DB-Batches, Rollup, Retention, Metadata, Tests.
- rust/Cargo.toml, rust/Cargo.lock für das neue Bin/whatlang.
- Deine neue Migration20260918123000_category_collector.sql (noch NICHT live), ops/systemd/twitch-runtime-roles.sql und neue Collector-Unit/Runbook.
- Falls tatsächlich nötig neuer Storage-Modul unter tb-analytics; bevorzuge jedoch deine begonnene Bin-Struktur. Serverseitige Admin-Lesequeries schreibt der Orchestrator gegen die Migration, kein API-Modul durch A nötig.
- REPORT.md zum eigenen Paket.

Du änderst NICHT mehr streams.rs, anon_chat.rs, scout_chat.rs, tb-monitoring/lib.rs/Cargo.toml, tb-chat, main.rs, tb-dashboard-api oder Frontend. Alle bisherigen eigenen Änderungen dort bleiben unangetastet, KEIN REVERT, KEIN STAGING dieser Pfade. Der Orchestrator besitzt und korrigiert sie jetzt. Frontend weiterhin B. Keine Unteragenten/Threads, keine Produktion/Deploy/main-Merge. Eigene exakte Dateien committen, kein add-A.

## Geteilter Chat-Vertrag (wird vom Orchestrator korrigiert)
Namen bleiben AnonChatConfig/AnonChatHandle/PrivmsgSink/TokioTaskSpawner. Einziger API-Unterschied: `#[async_trait::async_trait]` auf PrivmsgSink und Implementierungen, Methode jetzt `async fn handle_privmsg(&self,line:String)`. Dispatcher wartet bounded auf Sink, KEIN tokio::spawn pro Nachricht. Sink erhält PRIVMSG **sowie CLEARMSG/CLEARCHAT** im selben geordneten Ereigniskanal. Scout ignoriert letztere, Collector verarbeitet Löschereignisse. Zur PRIVMSG-Verarbeitung vorhandenen parse_privmsg weiterverwenden. Stats bisherige Felder bleiben vorhanden; zusätzliche Kontrollereignis-Drops separat möglich. Transport hat keinerlei ChatApi oder OAuth.

## Sofortige Korrekturen für A
REVIEW-NOTES H7/H8 beachten: retention_days DEFAULT90 als OBERGRENZE, nicht >=90/Mindest90. Rollup/Retention Pflicht und laufen auch bei pausierter Sammlung. IDs/Zeiten prüfen; SnapshotUNIQUE(poll_id,stream_id). source_room_id zusätzlich zu source_message_id für SharedChat. Seed142775730 bisher unbelegt; niemals blind benutzen. Der Orchestrator liefert additive sichere exakteKategorieauflösung im HelixTransport, Aufruf `resolve_category_id_exact("Deadlock").await -> Result<Option<String>,HelixError>`; nur exakten case-sensitiven Treffer nutzen, andere Spiele namensDeadLock nicht zusammenwerfen. ConfigID immer dagegen prüfen/aktualisieren bevor ersterPoll gespeichert wird. Neuer Kategorien-Abruf behält get_streams_by_category_full(...) und CategoryStreamsFetch, wird korrekt limitiert/validiert.

Rohtexte90Tage, Löschereignisse im gleichen Writer-Stream, persistenteRollups vorRetentionslöschung, kein durchPart/failedPollfalschesOffline. Keine eigene zweiteIRC/HelixImplementation, kein CrewGuard/ChatterTracker/ChatApi, keine ENV-Konfiguration. Bootstrap darf explizites geschütztes CLI-Konfigurations-/Credentialfile nutzen. Nicht legacyBotENVWrapper kopieren. OptionalesVOD/Clips-Enrichment darfKernloopnichtstundenlangblockieren. FehlendeFeatures ehrlichabgrenzenstattTabellenFeaturegleichsetzen.

## Orchestrator
Besitzt transportstreams.rs/monitoringanon_chat+lib/Cargo, ScoutAdapter, HauptbotZielschutz, AdminBackend(tb-dashboard-api). Die vorgeschalteten Reviewnotes H1-H6/H9 werden hier repariert/getestet. A muss diese nicht doppelt bauen. Code von A/B wird unabhängig geprüft; eigener Fixstand wird danach von B read-only mitgeprüft.

## B
Weiterhin ausschließlich bot/dashboard_v2 und FRONTEND-REPORT.md bis separat ein read-only Reviewauftrag kommt. API-CONTRACT bleibt gültig.
