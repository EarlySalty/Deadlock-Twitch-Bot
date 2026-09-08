status: aktiv
datum: 2026-09-08

# Bestand und Entscheidung

Graphify wurde vor der Codesuche im bestehenden Repository abgefragt. Basis des isolierten Worktrees ist origin/main d55880ab804086a39271fbd0885c7e0c50fe4c96.

Der historische Commit b8ae776f enthält einen ausführbaren tb-smalltalk-bench. Er hängt jedoch an tb-engagement, schreibt in die Produktions-Bench-Datenbank, verwendet Environment-Konfiguration und einen standardmäßig aktiven Cloud-Judge. Seine Vergleichsidee ist passend, die Ausführung erfüllt den heutigen privaten lokalen Vertrag nicht. Es wird deshalb der kleinste dateibasierte Replay-Einstieg innerhalb von tb-llm ergänzt; weder die Engagement-Laufzeit noch Sender werden eingebunden.

tb-llm hat bereits OpenAI-kompatible Body-Erzeugung, Fehlerklassifikation und Antwortauswertung. Die Produktionsfunktion weist andere Anbieter und Modelle ausdrücklich ab. Ein getrenntes, standardmäßig deaktiviertes Feature local-eval hält den genehmigten Versuch außerhalb dieser Produktionsauswahl und verwendet denselben Transport. Der Pfad benötigt weder Secretresolver noch Usage-Ledger.

Hardware: sichtbare 16 vCPUs, 48 GiB RAM, keine nutzbare GPU. Die Modelle werden nacheinander an 127.0.0.1:18789 geladen, Aliase qwen3.5-4b-local und qwen3.5-9b-local. Laufzeiten und Qualität sind vor dem Versuch nicht bekannt.

Die Daten werden getrennt, nur lesend und lokal exportiert. Account-Zuordnung ist kein Beweis manueller Autorschaft. Historische Audiosegmente haben einen belegten Endpunkt vor der Entscheidung, aber keine vollständig belegte damalige STT-Verfügbarkeit. Diese Grenze bleibt im Qualitätsmanifest sichtbar.
