# Frischer Fixer: Verbindungsende ohne Refresh-Möglichkeit

## Rolle und Zuständigkeit

Frischer Blatt-Fixer nach dem ersten inhaltlichen Merge-Gate-BLOCK. Keine zusätzlichen Worker, keine Review-Threads. Den bestehenden Branch und Worktree übernehmen, nichts neu bauen. Haupt-Orchestrator: `d3a1741e-82bc-4a48-865b-2845c663dca7`.

Repo `/home/nathanael/repos/Deadlock-Twitch-Bot`, Worktree `/home/nathanael/.worktrees/tb-social-token-ablauf`, Branch `fix/social-token-ablauf`. Codecommit `0ee53ca2`, geprüfter Stand `a062d624`; danach folgen Dokumentationscommits. Der Haupt-Checkout ist fremd verändert und bleibt unangetastet.

## Pflichtfix

Gate-Modell `gpt-6.1-sol`, BLOCKING unter `reauth.rs:67`: TikTok- und YouTube-Verbindungen ohne Refresh-Wert und ohne Refresh-Ende können ihr Access-Ende erreichen, ohne dass ein Vorfall oder eine DM entsteht. Der Dashboard-Status nennt sie bereits abgelaufen. Das Urteil steht in `REVIEW.md`.

1. Nach Graphify-Bestandssuche den gemeinsamen Verbindungsablauf im Sweep korrigieren. Instagram nutzt sein eigenes Access-Ende. Bei TikTok und YouTube mit Refresh-Möglichkeit bleibt die bisherige Refresh-Frist maßgeblich. Fehlt die Refresh-Möglichkeit, ist der Access-Ablauf das endgültige Verbindungsende. Vorwarnung und endgültiger Zustand müssen denselben existierenden Vorfall und DM-Pfad verwenden. Das bloße Vorhandensein des verschlüsselten Refresh-Feldes reicht für den hier beanstandeten Null-Fall; keine Secrets auslesen und keinen neuen Versandweg bauen.
2. Die Zwillinge prüfen: `credentials.rs` berechnet Ablauf und `reauth_soon`, `SocialMedia.tsx` wählt das angezeigte Datum. Auch dort soll ein nicht erneuerbarer Zugang den tatsächlichen Access-Ablauf als Verbindungsende verwenden. Das API-Feld `refresh_expires_at` bleibt das wirkliche Anbieter-Refresh-Ende und wird nicht mit einem erfundenen Datum befüllt. Die vorhandene Information `automatically_renewed` kann die Unterscheidung tragen. Zugänge mit gültiger Refresh-Möglichkeit dürfen wegen ihres kurzlebigen Access-Endes nicht wieder als abgelaufen erscheinen.
3. Die betroffenen Prüfungen nachziehen. PostgreSQL-Proben mit fester Uhr für beide Plattformen ohne Refresh-Möglichkeit: Vorwarnung in weniger als sieben Tagen, endgültiger Zustand beim Ablauf, genau ein Versand über Neustart und Eskalation. Bestehende verlängerbare TikTok-/YouTube-Fälle bleiben unverändert. Falls die Fixture nun das verschlüsselte Refresh-Feld lesen muss, vorhandene Fixtures passend ergänzen. Kein echter Produktionszugang als DM-Probe.
4. Format, Clippy, betroffene Tests und Dashboard-Build ausführen, Befunde und Nachweise aktualisieren. Bestehende API-Baseline: 1309 passed, 22 failed, identische Fehlerliste in `EVIDENCE.md`. Die neue Migration ist noch nicht produktiv angewandt; ihren vorhandenen Stand nicht unnötig ändern. Die Produktionshilfe enthält ihre SHA384-Checksumme.

## Nicht blockierende Anmerkung

Gesundes Instagram innerhalb von 30 Tagen zeigt den beauftragten Hinweis mit Datum, obwohl der automatische Refresh erst im Sieben-Tage-Fenster stattfindet. Der Gate nennt dies NIT und bittet um eine Bildprobe. Die Nutzer-Spezifikation schreibt weniger als 30 Tage ausdrücklich vor; dieses Fenster nicht eigenmächtig verändern. Zustand anhand des gebauten Artefakts oder DOM prüfen und die Anmerkung begründen. Der T3-Preview-Host war bisher nicht verfügbar; bestehende lokale Browserprüfungen können genutzt werden.

## Freigabe und Abschluss

Befunde dieser Gate-Runde gehen an diesen frischen Fixer, nicht zurück an den ersten Implementierer. Folgerunden werden vom unveränderten Merge-Gate mit demselben Modell wie dem ersten Codeurteil geprüft. Keine eigene Review-Delegation und kein Überspringen eines BLOCK.

Nach ALLOW gilt der ursprüngliche Abschlussauftrag: regulärer Merge und `git push origin HEAD:main`, manuelle Produktionsmigration als `postgres`, sauberer Release-Build mit `-j 2`, Herkunft gegen aktuellen `origin/main`-SHA, Deploy-Wrapper, Neustarts, Live-Beweis, Cleanup und Bericht. Die Details stehen in `TODO.md`. Kein eigenes Selbst-Settle vor dem belegten Abschluss.

Die eigene Test-DB liegt als Datenbank `tb_social_token_ablauf` in einem damals vorhandenen Wegwerf-Cluster auf Port 33045. Der Orchestrator hat inzwischen alte Testcontainer gestoppt. Vor neuen DB-Proben Verfügbarkeit prüfen; keinen fremden Container ungefragt starten oder stoppen. Der fehlgeschlagene eigene Containername ist `tb-social-token-ablauf-db`. Diesen beim endgültigen Cleanup stoppen und entfernen, wenn er noch existiert.
