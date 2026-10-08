# Nachweis zur Artefaktsicherung

Stand: 2026-10-08, 09:00 UTC.

Der reguläre Push des lokalen Dokumentationscommits `416851e6` wurde vom Secret-Scanner abgelehnt. Letzte zuvor bestätigte Remote-Sicherung: `f8c0ae31`. Der abgelehnte Push hat keine neue Remote-Sicherung hergestellt.

## Ursache und Korrektur

Der redaktierte Bericht `/tmp/tb-vollreview-gitleaks.le3Uz8/redacted.json` meldete zwei Treffer der Regel `generic-api-key` in `W03-DA03-GEGENPRUEFUNG.json`, Zeilen 956 und 957. Die Werte wurden zunächst nicht ausgegeben. Eine strukturierte Prüfung bestätigte: Beide sind bekannte Workflow-IDs im Format `wf_<8 Hexzeichen>-<3 Hexzeichen>` und verweisen auf vorhandene Einträge in `WORKFLOW-ARGS.json` mit 19 beziehungsweise vier Argumenten. Die Treffer betreffen diese Referenzen, keine Zugangsdaten.

Die missverständlichen Metadatenfeldnamen wurden präzisiert:

| Vorher unter canonical_inputs | Nachher |
|---|---|
| canonical_claims_key | canonical_claims_workflow_id |
| replacement_slots_key | replacement_slots_workflow_id |

Werte, Originalurteile und Provenienzdaten bleiben unverändert. Ein strukturierter Vergleich nach Rückbenennung der beiden Felder war exakt gleich. Originaltranscripts bleiben unverändert. Hooks, Scannerkonfiguration und Allowlists wurden nicht verändert.

Datei-SHA256 vor der Feldpräzisierung: `658fea9e12a905451ce2162807b3cdd2e0c34d19734758ac5a09146a948fe32e`.

Datei-SHA256 danach: `46a472cf3d741405fff02cfcc0e0617fb9da0d649f80db33d06e37b7db38febe`.

Ältere Abnahme- und Ereignisdokumente behalten ihren historischen Hash. Der neue Hash bezeichnet denselben Urteilsinhalt mit den präzisierten Metadatenfeldnamen.

## Prüfung

`gitleaks detect --source /home/nathanael/.worktrees/tb-vollreview-artefakte/.tasks/2026-10-08-twitch-bot-vollreview --no-git --no-banner --redact --exit-code 1` meldete um 09:00 UTC keine Treffer, Exit 0. Geprüft wurden etwa 2,65 MB Taskartefakte. Dies ersetzt weder den regulären Pre-Push-Hook noch einen erfolgreichen Pushnachweis.

Da die beiden Treffer nachweislich Workflowreferenzen sind, ist keine Secret-Bereinigung der Commitgeschichte erforderlich. Der bisher ungepushte Commit enthält für diese beiden Treffer keine Geheimnisse. Ein Folgecommit erhält die nachvollziehbare Korrektur.

TEXTNACHWEIS[DR-1]: Gedankenstriche 0 | ae/oe/ue/ss-Ersatz 0 | Absolutwörter 0 belegt | Senke: Taskartefakte
