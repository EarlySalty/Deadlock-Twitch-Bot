# Ursachenanalyse der Fallflut

Der Screenshot zeigt technische Ausfälle als Moderationsfälle. `LlmScamJudge` wandelte sowohl Aufruffehler als auch ungültige Antworten in `Verdict::unsure()` um. Dieses Urteil hatte 0 Prozent Sicherheit sowie eine leere Kategorie und Begründung. Der Gesprächswächter speicherte diesen Ersatz nach jeder weiteren Nachricht mit `action_taken = 'watching'`.

Der Rust-Fix gibt technische Fehler über `Result` zurück. Ohne gültige Entscheidung wird weder moderiert noch ein Fall gespeichert oder gemeldet. Der Dialog bleibt offen, damit eine spätere Nachricht nach der Erholung normal geprüft wird. Ungültige Modellantworten werden nicht ins Dialoggedächtnis aufgenommen. Echte inhaltliche `unsure`-Urteile bleiben erhalten.

## Live-Nachweise

Die Prüfung erfolgte über den lokalen PostgreSQL-Socket als PostgreSQL-Systemnutzer. Es wurden keine Zugangsdaten gelesen oder ausgegeben.

Am 3. Oktober waren seit dem 29. September 303 Zeilen mit `verdict = 'unsure'`, `confidence = 0`, leerer Kategorie, leerer Begründung und `action_taken = 'watching'` vorhanden. Der gesamte historische Bestand dieses technischen Ersatzurteils umfasste 746 Zeilen, erstmals am 28. Juli. Die letzte dieser Zeilen entstand am 30. September um 18:13 UTC. Echte Modellentscheidungen sind bis zum 2. Oktober um 21:30 UTC vorhanden.

Das Bot-Journal belegt am 29. und 30. September HTTP 404 mit `Model not found, inaccessible, and/or not deployed`. Der aktuelle zentrale Modellresolver prüft bereits täglich die freigegebene Flash-Familie und berücksichtigt 404/410 beim Prüfen verfügbarer Kandidaten. Seine persistierte Warnungsbegrenzung beträgt höchstens eine Meldung täglich und zwei wöchentlich. Eine Modelländerung war für diesen Fix nicht erforderlich. Die ursprüngliche technische Nichtverfügbarkeit ist im gemessenen Datenbestand seit dem 30. September beendet.

## Einmalige Datenbereinigung

746 technisch erzeugte Ersatzfälle wurden am 3. Oktober transaktional gelöscht. Das Prädikat war exakt:

```sql
verdict = 'unsure'
AND confidence = 0
AND category = ''
AND reasoning = ''
AND action_taken = 'watching'
```

Die Tabelle war während Zählung und Löschung mit `SHARE ROW EXCLUSIVE` gesperrt. Die Transaktion verlangte vorab exakt 746 Treffer und prüfte danach, dass der Löschumfang der Zählung entsprach. PostgreSQL bestätigte 746 entfernte Zeilen und `COMMIT`. Fälle mit echten Begründungen oder bereits erfolgten Maßnahmen blieben erhalten. Kein dauerhafter Nachtrag und kein Abfragefilter wurde eingeführt.

## Prüfung

Der Regressionstest simuliert HTTP 404 und kaputtes JSON, verlangt jeweils null gespeicherte Fälle und null Moderationsmaßnahmen und prüft danach dieselbe Unterhaltung mit wieder funktionierender Modellantwort. Bestehende Parser-, Dialog- und Datenbank-Mocks wurden an die Fehlertrennung angepasst.

## Warnungen bei weiteren Ausfällen

Die Abnahme fand zusätzlich ungebremste Warnungen im zentralen Connector und eine doppelte Warnung beim Gesprächswächter. Der Connector und die Antwortprüfung verwenden nun den bereits vorhandenen `tb_observability::warning_budget::warn`-Pfad. Dieser begrenzt gleiche Fehlerklassen im laufenden Prozess auf eine Meldung je 24 Stunden und zwei je sieben Tagen und führt die unterdrückten Wiederholungen bei der nächsten Meldung mit. Die lokale doppelte Aufrufmeldung steht nur noch auf Debug. Das bestehende Warnbudget ist ein Prozesszustand und wird bei einem Dienstneustart zurückgesetzt; der separate tägliche Modellresolver behält seine persistierte Begrenzung.

## Stand der Abnahme

Beide ursprünglichen Rust-Dateien und der zusätzliche Connector-Diff bestanden die gezielte `rustfmt --edition 2021 --check`-Prüfung. `git diff --check` war ebenfalls grün. Der Selbstreview über `gate_hook.py --review --base origin/main --head fix/scam-fallflut` lieferte für beide Durchgänge `ALLOW`.

Der Testlauf `cargo test -p tb-chat --lib conversation_scam` wurde während der Abhängigkeitenkompilation nach der neu übermittelten Hostregel geordnet mit Exit 130 beendet. Es liegen daraus keine Testresultate vor. Der eigene Cargo-Prozess war danach beendet. Die abschließenden Compilerprüfungen müssen im integrierten Stand unter den beiden Sperren aus `Documents/.tasks/2026-10-02-offene-branches/HOSTPROBE.md` stattfinden. Der Worker startete keinen weiteren Compiler.
