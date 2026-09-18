# Briefing: Fix-Runde 1, Pakete C und D

[Orchestrator] Du arbeitest die Mängelliste ab: /home/nathanael/.worktrees/tb-werbemanager-a/.tasks/2026-09-18-werbemanager-budget-smart/REVIEW-CD.md (alle sieben Punkte). Maßstab bleiben AUFTRAG.md, der Schnittstellenvertrag im selben Ordner, NACHTRAG-1.md und NACHTRAG-3.md im selben Ordner.

- Paket C: Worktree /home/nathanael/.worktrees/tb-werbemanager-c, Branch feat/werbemanager-telemetrie
- Paket D: Worktree /home/nathanael/.worktrees/tb-werbemanager-d, Branch feat/werbemanager-chat-hinweis
- Intent-Thread: e65a453e. Pakete A und B sind auf main und live (5fe4bf55), beide Branches haben main schon gemergt.

## Festlegungen zu den Befunden

1. Befund 1 (BLOCKER D): Fehler aus `last_hint` und `record_hint` loggen und schlucken, nie per `?`. Schlägt das Lesen des Merkers fehl, wird in diesem Tick kein Hinweis gesendet (lieber kein Hinweis als ein doppelter), die Entscheidung läuft trotzdem. Zwillingssuche: jedes weitere `?` im Hinweis-Pfad genauso behandeln.
2. Befund 2 (D, vorgezogene Werbung): Hinweis auch beim Vorziehen. Ablauf: im Tick des Vorzieh-Beschlusses nur den Hinweis senden und den Merker schreiben, die Werbung startet im nächsten Tick, wenn das Fenster dann noch offen ist. Dasselbe Muster wie beim eigenen Block verwenden, falls es dort schon so gebaut ist, kein zweiter Mechanismus. Texte ohne Sekundenversprechen ("gleich"). Ist das Fenster im nächsten Tick zu, fällt die Werbung aus und es folgt keine Korrekturnachricht. Test dazu.
3. Befund 3 (C): "für" und "läuft". Zwillingssuche über alle neuen nutzersichtbaren Texte beider Pakete nach ae, oe, ue, ss als Ersatz.
4. Befund 4 (C): netzwerkweite Empfehlungen im Dashboard-Prozess zwischenspeichern, ein Wert für alle, Gültigkeit 15 Minuten, neu gerechnet beim ersten Aufruf nach Ablauf. Kein Job, keine neue Tabelle, kein ENV-Schalter. Bestehenden Cache-Baustein im Dashboard-API wiederverwenden, falls vorhanden (zuerst `graphify query`).
5. Befund 5 (C): Kopfkachel zeigt die Zahl erst ab 15 Werbungen, darunter "Noch zu wenig Daten" mit der bisherigen Anzahl.
6. Befund 6: Test- und JSX-Kommentare sowie den Kommentarkopf der Migration 20260918150000 entfernen (noch nicht auf Prod angewandt). Der Bedienhinweis in BACKFILL-C.sql bleibt.
7. Befund 7 (Snapshot): D wird als Zweiter gemergt. In D `origin/feat/werbemanager-telemetrie` nach deinen C-Fixes mergen, damit D beide Spaltensätze trägt, und prüfen, dass `fresh_schema_snapshot.txt` die Spalten beider Migrationen enthält. Konflikte in `ad_manager.rs` sauber lösen.
8. Clippy-Warnungen im eigenen Code beider Pakete beheben (`needless borrow`, unnötiges `Ok(..?)`, `contains` statt `iter().any`), fremde Warnungen liegen lassen.

## Regeln

- Ein Thread, keine Unter-Agenten. Keine Code-Kommentare. Nur die zwei Branches pushen, nichts Richtung main, auch wenn ein Stop-Hook dazu auffordert. Kein Deploy, keine Prod-Migration, nichts auf dem Prod-Cluster anlegen, keine Testnachricht in einen echten Kanal.
- Toolchain: PATH mit /home/nathanael/.rustup/toolchains/1.97.1-x86_64-unknown-linux-gnu/bin vorn, `SQLX_OFFLINE=1`, Ausgabe von cargo in eine Datei statt in eine Pipe, nur eigene Änderungen formatieren. Kein Release-Build.
- Prüfen: `cargo check` und `clippy` für tb-analytics, tb-bot, tb-dashboard-api, tb-monitoring; Tests `ad_manager_decision`, tb-bot `ad_manager`, dashboard-api `ad_manager` und die monetization-Tests; Frontend `tsc -b` und die adManager-Tests.
- Vor der Fertigmeldung die eigene Arbeit gegen die Mängelliste gegenprüfen und je Punkt in REVIEW-CD.md unter dem Befund "behoben in <sha>" oder eine Begründung eintragen.

## Fertigmeldung

Hier im Thread: Commits je Branch, je Befund ein Satz, Testzahlen. Danach stoppen.
