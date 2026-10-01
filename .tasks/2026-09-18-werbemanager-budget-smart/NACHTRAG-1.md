# Nachtrag 1 (2026-09-18): Twitch-Werbemanager ergänzen statt ersetzen

Leitgedanke des Nutzers: der Twitch-eigene Werbungs-Manager bleibt, wir liefern das, was ihm fehlt: den klugen Zeitpunkt. Der Nutzer hat bei Twitch einen Werbeplan aktiv (89 Werbungen in 30 Tagen, im Schnitt 32 Sekunden, fast alle automatisch).

## Änderung für Paket A: Budgetquelle wird erkannt, nicht gefragt

- Liefert Helix einen Werbeplan (`next_ad_at` gesetzt), ist Twitch die Budgetquelle. Der Bot legt keine eigenen Blöcke obendrauf, sondern bewegt die geplante Werbung: vorziehen in ein Fenster (Werbung in der Länge der geplanten selbst starten, Twitch plant danach neu) oder per Pause aus einer Sperre herausschieben. `budget_minutes_per_hour` gilt dann nicht.
- Liefert Helix keinen Plan, gilt das eigene Budget mit 30-Sekunden-Blöcken wie im Auftrag.
- Vorziehen ist neu: bisher handelt `decide()` erst im Vorlauf von 60 Sekunden. Künftig darf er eine geplante Werbung vorziehen, sobald ein Fenster offen ist und die geplante Werbung in die nächste Sperre fallen würde (etwa Queue jetzt, Werbung in 4 Minuten fällig, ein Match dauert länger). Vorziehen nur, wenn Mindestabstand und Helix-Sperrzeit es erlauben.
- Status bekommt `plan.source: 'twitch' | 'own'`. Neue Grundcodes `pulled_forward` und `twitch_plan_active` in den API-Vertrag eintragen.
- Ob ein selbst gestarteter Block die geplante Twitch-Werbung wirklich verschiebt, am echten Helix-Verhalten prüfen (`next_ad_at` vor und nach dem Start vergleichen, Werte ins Verlaufs-Detail), nicht annehmen.

## Änderung für Paket B

- Bei `plan.source = 'twitch'` ist das Budgetfeld nur Anzeige: "Dein Budget kommt aus dem Twitch-Werbungs-Manager" plus die Werte aus dem Plan. Eingabe nur bei `'own'`.

## Neu: Paket C, Telemetrie und Auswertung (startet erst nach Fertigmeldung von A)

Ziel: lernen, wann Werbung am wenigsten schadet, und den Algorithmus später daran nachschärfen. Erst sammeln und ehrlich auswerten; der Entscheider ändert sich durch C nicht von selbst.

1. Kontext beim Schreiben festhalten: `store_ad_break_event` (`rust/crates/tb-monitoring/src/telemetry.rs:154`, Tabelle `twitch_ad_break_events`) bekommt je Werbung den Moment dazu: Match-Zustand und Sekunden seit Match-Beginn oder Match-Ende, Chat-Nachrichten der letzten Minute und der letzten fünf, Zuschauer davor, Raid und Erstchatter im Sperrfenster ja oder nein, Quelle (Twitch-Plan, Bot vorgezogen, Bot eigener Block, manuell), im Fenster ja oder nein. Verknüpfung zur Entscheidung aus `twitch_ad_manager_decisions`. Kein Nachtrag-Job, kein Reparieren pro Poll.
2. Wirkung messen: Zuschauer und Chat-Tempo bei plus 1, 3 und 5 Minuten, dazu die Erholungszeit. Als Vergleich dieselben Messpunkte für werbefreie Momente derselben Sendung, damit der allgemeine Trend des Streams herausgerechnet wird. Heute zeigt die Auswertung im Schnitt "plus 11,9 Prozent Zuschauer nach Werbung"; das ist Streamwachstum, keine Werbewirkung.
3. Bestehende Auswertung unter Analyse, Tab Monetization, reparieren (`bot/dashboard_v2/src/pages/Monetization.tsx` und der zugehörige Handler): Vorzeichen in den Empfehlungstexten sind verdreht ("Manuelle Ads verlieren -2992% weniger Viewer", "geringster Drop -56,7 %" bei einem Plus), und Empfehlungen entstehen aus Einzelfällen (1x). Empfehlung nur ab einer Mindestzahl je Gruppe (Vorschlag 15), sonst "noch zu wenig Daten". Neue Aufschlüsselung nach Moment: Queue, erste Match-Minute, im Match, nach Matchende, ruhiger Chat, aktiver Chat.
4. Netzwerkweit auswerten, je Streamer anzeigen: die Lernbasis sind alle Partner mit aktivem Werbemanager, im Dashboard sieht jeder nur seine Zahlen.
5. Altdaten: ein einmaliger SQL-Backfill des Kontexts für vorhandene `twitch_ad_break_events` aus Chat-Log und Session-Daten ist erlaubt, soweit die Quelle es hergibt; Match-Zustand gibt es rückwirkend nicht und bleibt leer.

Dateien von C überschneiden sich nicht mit B. Mit A teilt C die Signalquellen, deshalb startet C auf dem Branch-Stand von A.
