# Unabhängige Abnahme

Stand: 2026-09-12, Basis b440e84a, finaler Code-SHA ce9e7450b795de17f5e21ed0e5aaf4361cdb198a (86c1b1d3 plus P2-Nachtrag). Reviewer: Codex intent_review, unabhängig vom Autor Opus48. Keine Produktcodeänderung durch den Reviewer. Der finale promos.rs-Blob 802cca6b entspricht exakt dem bereits nachgeprüften Zusatzdiff.

## Urteil

Finale inhaltliche FREIGABE: J. Weiterer Fix nötig: N. Compiler und gezielte Tests des finalen Codes sind nachgewiesen. Clippy nach dem P2-Nachtrag und das reguläre Merge-Gate bleiben technische Abschlussprüfungen der ausführenden Session. Kein eigener Cargo-Lauf, um den Hostslot nicht doppelt zu belegen.

Timer, Chat-Aktivität und Zuschaueranstieg prüfen dieselben eingestellten Grenzen. Die Community-Ausnahme ist entfernt; die Identitätsauflösung und kanalspezifischen Werte bleiben erhalten. Auch Erstwerbung prüft die eingestellte Zahl neuer Chatter. Zeit und Aktivität sind UND-verknüpft. Ein ruhiger Chat ohne ausreichende Nachrichten bleibt gesperrt. Die neue Admin-Textquelle und ihre Themenrotation bleiben erhalten. Keine durch den Diff eingeführte Sicherheitslücke gefunden.

## Gefundener und behobener P2

In 86c1b1d3 zählte get_new_chatters_in_window_inner (promos.rs:2151) Aktivitätsbucket plus Session-Viewer. update_seen_chatters_inner (promos.rs:2025) markierte nach erfolgreicher Werbung nur den Bucket als gesehen. Beispiel: A schreibt acht Nachrichten, B/C sind reine API-Viewer; nach der ersten Werbung war nur A als gesehen markiert. Nach erneut acht Nachrichten von A und abgelaufenem Zeitabstand zählten B/C erneut als zwei neue Teilnehmer, obwohl niemand hinzugekommen war.

Der Nachtrag lädt in mark_promo_sent die Session-Viewer vor dem State-Lock und übergibt sie an update_seen_chatters_inner. Dort wird nun dieselbe Union als gesehen gespeichert. Die vorhandene Definition der API-Viewer und die Ablaufregel bleiben unverändert. Der konkrete Resetfehler ist damit behoben.

Der gezielte Regressionstest folgewerbung_ohne_neuzugang_bleibt_gesperrt setzt A im Bucket und B/C in die Sessiontabelle, prüft zunächst Bereitschaft, markiert den Versand, erfüllt anschließend erneut Nachrichten und Aktivitätsabstand und erwartet ohne neue Teilnehmer eine Sperre. Er isoliert damit den beanstandeten Neuzugangscheck.

## Verifikation

Die folgenden Logs wurden vom Reviewer selbst gelesen:

- /tmp/testbuild.log: finaler Code erfolgreich im Testprofil kompiliert, 4 min 19 s.
- /tmp/t_regression.log: folgewerbung_ohne_neuzugang_bleibt_gesperrt bestanden; 1 bestanden, 0 fehlgeschlagen, 0 ignoriert.
- /tmp/t_community.log: kanalspezifische Werte sowie Persistenz/Farbauswahl/Sendpfad bestanden; 2 bestanden, 0 fehlgeschlagen, 0 ignoriert.
- /tmp/t_activity.log: Aktivitätschecks bestanden; 3 bestanden, 0 fehlgeschlagen, 0 ignoriert.
- /tmp/check2.log und /tmp/clippy2.log: erfolgreich, aber vor dem P2-Nachtrag. Diese Logs sind ausdrücklich kein Clippy-Nachweis für ce9e7450.

Vorbestehender repo-weiter Rustfmt-Drift ist kein inhaltlicher Befund dieses Reviews; Basisvergleich wird vom Worker dokumentiert. Keine neue offene Abweichung bei Nachprüfung der bekannten Mängelliste.
