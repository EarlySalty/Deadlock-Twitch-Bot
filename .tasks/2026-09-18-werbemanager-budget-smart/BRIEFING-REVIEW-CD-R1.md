# Briefing: Review Runde 1, Pakete C und D gemeinsam

[Orchestrator] Du bist Reviewer, nicht Autor. Du änderst keinen Code. Ergebnis ist eine vollständige Mängelliste in `/home/nathanael/.worktrees/tb-werbemanager-a/.tasks/2026-09-18-werbemanager-budget-smart/REVIEW-CD.md` (nur diese Datei schreibst du, nicht committen).

## Gegenstand

- Paket C (Telemetrie, Auswertung): Worktree /home/nathanael/.worktrees/tb-werbemanager-c, Branch feat/werbemanager-telemetrie, Diff `git diff origin/main...HEAD`
- Paket D (Chat-Hinweis vor Werbung): Worktree /home/nathanael/.worktrees/tb-werbemanager-d, Branch feat/werbemanager-chat-hinweis, Diff `git diff origin/main...HEAD`
- Maßstab: AUFTRAG.md, der Schnittstellenvertrag im selben Ordner, NACHTRAG-1.md (Paket C), NACHTRAG-3.md (Paket D) im Akten-Ordner. Pakete A und B sind schon auf main und live, sie sind nicht Gegenstand.

Beide Branches zusammen prüfen: sie landen nacheinander auf main und fassen teils dieselben Dateien an (`ad_manager.rs`, Schema-Snapshot, Migrationen). Melde, wo der zweite Merge kollidiert.

## Prüfpunkte

1. Abnahme gegen den Nutzer-Wunsch, fertig ja oder nein. C: je Werbung wird der Moment festgehalten (Match-Zustand, Chat-Tempo, Zuschauer, Raid, Erstchatter, Quelle), Auswertung zeigt ehrlich, wann Werbung am wenigsten schadet, Empfehlungen erst ab 15 Werbungen je Gruppe, Vorzeichen richtig, kein n=1-Urteil. D: eine Chat-Zeile kurz vor der Werbung, nur wenn der Werbemanager an ist und der Schalter an ist, rotierende Varianten, kein LLM.
2. Jede SQL-Abfrage gegen den Schema-Snapshot `rust/crates/tb-db/tests/fresh_schema_snapshot.txt` prüfen: Spaltentypen, Funktionen auf falschem Typ, fehlende Spalten. In Paket A brach genau so eine Abfrage (`BTRIM` auf timestamptz) den ganzen Tick ab. Ein Fehler in Telemetrie oder Hinweis darf den Entscheider nie abbrechen: prüfe jedes `?` im Tick-Pfad.
3. D: Idempotenz des Hinweises (Neustart, doppelter Tick, Lease), kein Hinweis ohne folgende Werbung als Dauerzustand, kein Hinweis bei ausgeschaltetem Manager, gesendet über den bestehenden Chat-Sendeweg. Texte: echte Umlaute, keine Em-Dashes, nicht bottig, keine Behauptung, dass alle die Werbung sehen.
4. C: kein Endpunkt gibt fremde Kanaldaten heraus, Identität nur aus der Session, netzwerkweite Werte nur aggregiert. Kein Nachtrag-Job, kein Reparieren pro Poll. Dashboard: Farben nach Regel (Bronze-Gold-Skala), ehrlicher Leerzustand, kein internes Vokabular.
5. Migrationen: C 20260918140000, D 20260918150000, keine Kollision, keine Änderung an angewandten Migrationen (20260918120000 ist auf Prod), Rechte für `twitchbot` und `twitchdash` wo neue Tabellen oder Sequenzen entstehen.
6. Keine neuen Code-Kommentare, keine harten Timeout- oder Modell-Konstanten, keine ENV-Schalter.

## Format von REVIEW-CD.md

Je Mangel: Nummer, Schwere (BLOCKER, MANGEL, NIT), Paket, `pfad:zeile`, was falsch ist, was stattdessen gelten muss. Oben ein Urteil: FREIGABE oder FIX NÖTIG, dazu die Abnahme-Antwort fertig J/N je Paket. Keine Unter-Agenten. Codebase-Fragen zuerst über `graphify query`. Danach hier im Thread kurz melden und stoppen.
