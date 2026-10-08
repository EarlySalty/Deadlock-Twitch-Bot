# W03: Nachweis des Loginbereichs DA03

Feste Codebasis `0ecae1370f1a80d1a101249b5c932663d69be8af`. Defektworkflow `wf_bd410bd5-531`; ausgewertet sind ausschließlich DA03-S001 bis S006 in fünf Linsen. Die übrigen W03-Bereiche laufen unabhängig weiter.

## Ergebnis und Abdeckung

- 30 vollständige Rückgaben, zwölf Primärdateien, 9111 Primärzeilen und 45555 Zeilen-/Linsenpaare. Die deklarierten Intervalle decken das Manifest rechnerisch ab. Das beweist weder tatsächliche vollständige Werkzeuglektüre noch Fehlerfreiheit.
- 37 Rohmeldungen, fünf interne Duplikate, 32 Gruppen. Sechs Gruppen verweisen auf bestehende W02-Claims. 26 Gruppen sind neu: sechs A-, 13 B- und sieben C-Vorschläge.
- Die neuen A/B-Vorschläge sind ungeprüft. Gegenprüfung in `wf_97f7401d-6c8`, Task `w4yxi8vtu`, Script `tb-vollreview-da03-gegenpruefung-wf_97f7401d-6c8.js`. Je zwei frische Sol-Skeptiker erhalten ausschließlich Claim und Ort. Eingaben in WORKFLOW-ARGS.json.

W03-DA03-KANDIDATEN.json enthält jeden ursprünglichen Befund unverändert unter raw_findings, Quellenverweise, Gruppen und Grenzen. Die 37 Roh-IDs sind genau einmal einer neuen oder verknüpften Gruppe zugeordnet. Bestehende W02-C-Sperren bleiben erhalten; ähnliche neue Claims wurden nicht allein wegen derselben Datei zusammengelegt.

## Modell- und Herkunftsprüfung

Konsolidierer `a120b32523dd1f51a`, Workflow `wf_7839ce50-df2`: 68 echte Sol-Datensätze, SHA256 `e89fb771f74eadb7bdff9baa12ab55d07bc5f4fdb0550381325caf3defed7334`. Astra hat Modellfelder und Hash des fertigen Transcripts geprüft.

Astra wiederholte außerdem die Prüfung der 30 ausgewählten Reviewertranscripts: 1393 echte Datensätze mit `message.model=gpt-6.1-sol`, keine fremden Modelle, passende Transcript-Hashes, je ein finales StructuredOutput mit exakter JSON-Übereinstimmung zum Journal und passendem Inhaltsdigest. Keine Abweichung. Einzelbelege stehen in model_proofs.runs samt run_columns der JSON-Datei.

Das Quelljournal wird durch andere W03-Bereiche weiter ergänzt. Sein Gesamthash ist daher ein Beobachtungsstand. Stabile Belege sind die ausgewählten 60 Start-/Ergebnisdatensätze und die abgeschlossenen individuellen Transcripts; keine Behauptung eines unveränderlichen Gesamtjournals.

## Vorherige Gegenprüfung

Bei Beginn der Konsolidierung enthielt W02-GEGENPRUEFUNG-03.json vier ausgefüllte Paare und 18 Platzhalter. Der Konsolidierer hat deshalb die acht für seine Verknüpfungen benötigten fertigen W02-Urteile direkt am ursprünglichen Journal und den passenden Transcripts geprüft. Diese dokumentierte Einschränkung ist historisch; der spätere Exportstand änderte sich weiter und wird getrennt vervollständigt. Platzhalter wurden nicht als Urteile gewertet.

Verknüpfte Claims: Fingerprint-Überschreiben, Affiliate-Login-CSRF, fehlgeschlagene lokale Logout-Löschung, Wiederanlage durch Partner-Refresh, verlassene Dashboard-OAuth-States und Partner-Link-CSRF. Unterschiedliche Widerrufs- oder Loginpfade bleiben eigenständige Claims, wenn Ursache und Fehlervertrag abweichen.

## Grenzen

Keine ausgeführten Tests, Builds, Datenbank-, Netzwerk- oder Browserprobes durch die Reviewer oder den Konsolidierer. Keine neue Quellprüfung durch die mechanische Modellabnahme. Laufzeitkonfiguration, externe Broker-/Proxy-Verträge und Wartungsaufgaben behalten ihre ursprünglichen Einschränkungen. Aus der Konsolidierung folgt keine Fix-, Merge- oder Deployfreigabe.
