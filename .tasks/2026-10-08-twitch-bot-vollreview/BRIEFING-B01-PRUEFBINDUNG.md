# B01: Prüfbindung schließen

Frische Sol-Prüfrolle, kein Anwendungscode-Fix und kein neuer Reviewer. Worktree `/home/nathanael/.worktrees/tb-vollreview-idempotenz`, Head `c3aa3cc94fec60aa365dcc9ca9f54b4bd3f1539a`, bisherige Basis `a8b5b5e986a1de0b8e2f981651f83bda9cf400dd`. Keine Quelländerung, kein Rebase, Commit, Push, Merge oder Deploy in dieser Rolle. Die frisch bestätigte B02-Integration auf main ändert den unveränderten B01-Quellbaum nicht; Integrationsabgleich folgt getrennt.

## Nachweislücke

Frischer Kritiker a014f2fa4705be9bd gab fachlich ALLOW, stellte aber klar:

- `/tmp/tb-b01-sol-3341098f-regression.log` enthält einen bestandenen Test; der Commit 3341098f enthält selbst weder Fix noch Regressionstest. Der damalige Lauf könnte auf uncommitteten Änderungen beruhen. Der Dateiname beweist den getesteten Quellstand nicht.
- `/tmp/tb-b01-sol-c3aa3cc9-regression.log` ist leer. Der vorige neue Versuch wartete 3600 Sekunden erfolglos auf einen Slot. Daraus kein neuer Testnachweis.
- Ältere Suite: 336 bestanden, 24 gleich benannte Baselinefehler. Clippy mit fremder Diagnose, Paketformatierung laut Fixer 34 Bestandsabweichungen. Diese Angaben sind nicht durch den Lognamen an den Fix gebunden.

Zuerst vorhandene fertige Agenten-/Werkzeugnachweise aus `wf_45aca23b-0b9` und Vorgängern gezielt auf belastbare Bindung prüfen. Ein alter Logname, das Dateialter oder ein aktueller identischer Diff allein reichen nicht. Ein echter damaliger Quellhash, ein festgehaltener vollständiger Diff oder die nachvollziehbare lückenlose Werkzeugfolge können geeignet sein. Keine vollständigen Transcripts ausgeben, keine Secrets/ENV-Dateien lesen.

Ist die damalige Bindung nicht beweisbar, passende neue Prüfung am unveränderten aktuellen Head ausführen. Vorher HEAD, sauberen Arbeitsbaum, Quell-/Diffhash und konkretes Kommando festhalten; nachher erneut vergleichen. Regressionsfilter aus der eigenen Quelle ermitteln, nicht raten. Bestehende Crate-Prüfpflicht einschließlich Tests, Formatierung und Clippy gilt; vorhandene sauber belegte unveränderte Baseline darf wiederverwendet werden. Rote Baseline ehrlich ausweisen. Ohne DB-Opt-in übersprungene Tests sind kein DB-Nachweis.

Vorher nach eigener laufender Prüfung suchen, keinen Doppelbuild. cargo-slot verwenden, keine fremden Slots/Prozesse verändern. Normale Kompilierung nicht nach einer pauschalen Frist abbrechen. Ein erneuter Slotblocker muss auf tatsächlichen Zustand gestützt sein, nicht auf einen blinden Stundenretry. Toolchain 1.97.1, SQLX_OFFLINE=1, Kompilierung --jobs 1; erlaubte synthetische Test-DB gemäß AUFTRAG.md. Keine produktiven Datenbanken, Migrationen, Kontoaktionen oder Browserarbeit.

## Ausgabe

Erlaubte Schreibdatei: `B01-PRUEFBINDUNG.md` im Taskordner, Prüfprotokolle im eigenen neuen Verzeichnis unter `/tmp`. Pro Prüfung: Kommando, Ausgang, geprüfter Quellstand, Logpfad und SHA256, Bindungsbeweis, verbleibende Lücken. Die fachliche Kritik bleibt unverändert. Kein neues ALLOW aus einem technischen Prüflauf ableiten. Keine Fragen, Delegation oder Sessionnachrichten.
