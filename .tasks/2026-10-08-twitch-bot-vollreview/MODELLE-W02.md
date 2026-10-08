# Modellnachweise W02

Session: `f61905e7-f7ff-405b-a6d7-090dec371fcb`. Transcript-Basis wie in MODELLE.md. Diese Liste enthält vorerst nur die abgeschlossenen Reviewer, deren erste Kandidaten bereits weitergegeben oder erfasst wurden.

| Rolle | Workflow | Agent | Modell | Nachrichten | Transcript-SHA256 |
|---|---|---|---|---:|---|
| DA01-S004 Nebenläufigkeit | wf_cdc4c5ac-9bb | af1906e0e42b5f7a4 | gpt-6.1-sol | 42 | 71d7637553660d13912ab3100e3a4979b62a4f23cc23796d6288b70e3c64d36e |
| DA01-S003 Ressourcen | wf_cdc4c5ac-9bb | a38ff8d6152375fd2 | gpt-6.1-sol | 49 | c4cc0825e31bad7977fab6dbd30b09f2f6fd2c2d47714cd0d3214ebd8c6d874c |
| DA01-S003 Korrektheit | wf_cdc4c5ac-9bb | a5d8a2b501d115ffc | gpt-6.1-sol | 61 | 4ee266e1a9504da20f5e2516fec933d6768d9acebfd9c2823a6dc3d5c134fba1 |

Die abgeschlossenen Transcripts wurden vollständig nach `message.model` ausgewertet. Andere Modelle wurden darin nicht gefunden. Skeptiker der beiden B-Kandidaten laufen in `wf_3b16d4aa-68e`; deren Nachweise stehen noch aus.

## Abgrenzung der Wiederaufnahmen

Der Workflow wurde mehrfach fortgesetzt. Sein Journal enthält deshalb auch frühere, blockierte Rollenversuche. Seit der Scriptänderung `2026-10-08T02:02:26Z` startet jeder Reviewer als `general-purpose`. Die Kontrollauswertung um 02:19 UTC fand 36 neue Sol-Agenten, darunter 22 vollständige Rückgaben. In diesen neuen Transcripts wurde kein Cargo-Prüfaufruf gefunden. Alte `rust-reviewer`-Fehlversuche zählen weder als Abdeckung noch als negatives Review-Ergebnis.

Bei der Endauswertung je Kombination aus Abschnitt und Blickwinkel den erfolgreichen aktuellen Rollenversuch verwenden. Mehrere Versuche nicht addieren. Ein fehlendes oder unvollständiges Resultat bleibt eine Lücke.
