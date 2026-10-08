# Serielle kleine Integrationen: B05, B09, B08

Dieser neue Auftrag ist eine Integrationsrolle, keine Fortsetzung der früheren nur vorbereitenden Rollen. Der ursprüngliche Nutzerauftrag in AUFTRAG.md, Abschnitt 6, beauftragt kleine Merges und Pushes. Hier sind genau die drei folgenden bereits fachlich abgenommenen Patches zur mechanischen Integration freigegeben, wenn ihre aktuellen Nachweise und der lokale Gate stimmen. Kein neuer Anwendungscode. Deploy bleibt gesperrt.

Die Rollen laufen streng nacheinander: zuerst B05, nach dessen belegtem Main-Push B09, danach B08. Je Paket frischer Sol-Kontext. Bei echter Blockade hält die Kette an; weitere Pakete werden nicht gestartet. Astra führt während dieser Kette keinen anderen eigenen Main-Push aus. Artefaktsicherungen auf dem eigenen Auditbranch sind davon unabhängig.

## Erhaltene Pakete

| Paket | Worktree unter /home/nathanael/.worktrees/ | Erhaltener Head auf e98b7f01 | Patch |
|---|---|---|---|
| B05 | tb-vollreview-plattform-refresh | 6383a00f031750095a38ff25809aee75a95cf25d | handlers/platform_token.rs, handlers/plattform_oauth.rs |
| B09 | tb-vollreview-idor-fixture | fa01de7b12f51bde4a9af922ae3e3bcf7ee72598 | auth/idor_e2e_tests.rs |
| B08 | tb-vollreview-query-grenzen | 70d26a8b00189d3212af53337935757eb576a50e | query_int.rs, handlers/admin_research.rs |

Pfade relativ zu rust/crates/tb-dashboard-api/src. Aktuell bestätigtes main ist A01 f04c0ef03d47ce4893ee4d17cb98eeb4c2f6c473. Nicht voraussetzen, dass dies beim eigenen Start unverändert gilt. Kein anderer Quellwriter ist für diese drei Paketdateimengen aktiv.

## Bereits abgenommene Herkunft

B05: echte fachliche Kritik in ABSCHLUESSE-08.json. Abgeschlossene Vorbereitung in B05-INTEGRATIONSVORBEREITUNG.md und ABSCHLUESSE-11.json. Vorbereitungsrolle a4b8afb3ceb6d6834, 119 echte Sol-Datensätze, Transcript-SHA256 67c0d29903dbb025c54962442f44231e5ae8278992966022bbbec9e5c5dbed6c. Unveränderter Originalpatch, vollständige Suite auf B02-Basis 1337/35 gegen 1334/35, gleiche vollständige Fehlerkörper. 40 Fokusfälle und 50 OBS-Fälle bestanden, Clippy Exit 0. Historische Baselinebindung geschlossen. Sol-Gate ALLOW gilt für das alte feste Paar, nicht automatisch nach A01.

B09: echte fachliche Kritik in ABSCHLUESSE-07.json. Vorbereitung in B09-INTEGRATIONSVORBEREITUNG.md und ABSCHLUESSE-11.json. Vorbereitungsrolle a0312a5f93405be98, 127 echte Sol-Datensätze, Hash 6abcf2c84507f0186f009b4c7452755f56f82a2859f2d2d2ec404bfc9376e1f3. Zwei synthetische Fixture-IDs, keine produktive Authänderung. Neue Standard-Suite 29 gegen 31 Baselinefehler, genau zwei IDOR-Fehler entfernt. Vier normale Tests und zwei Doctests ignoriert, kein positiver Doctestnachweis. Fokus 2/2, Clippy Exit 0, alter Sol-Gate ALLOW.

B08: tatsächlicher nichtleerer Diff und frische fachliche Kritik ALLOW in ABSCHLUESSE-13.json. Abschlussrolle ab2d78f0f75628752, 133 Sol, Hash 0f2fb72606332345b89f9b1a61e6effdcf23b882bcf7e06541617530d61b9b48. Kritiker af0a0937bfc225e28, 33 Sol, Hash fb0409bf99fa9beaaa119c69546a4677e4fed46c676ef13d10e86c7ca71f8b2c. Beide durch Astra geprüft. B08-PRUEFABSCHLUSS.md: ursprünglicher Patch nach Abbruch unverändert erhalten, Standard-Suite 1337/31/6 ignoriert gegen 1332/31/6, 13 Parserprüfungen ohne DB bestanden, Research-Ablehnungsvertrag erhalten. Clippy beidseitig Exit 0. DB-gestützter Research-HTTP-Test überspringt bei fehlender optionaler Konfigurationsdatei; sein grüner Eintrag ist kein HTTP-/DB-Nachweis. Kein neuer Syntax- oder Research-Vertrag.

## Verbindlicher Integrationsweg je Rolle

1. Eigenen tatsächlichen Zustand und bereits laufende eigene Prüfungen feststellen. Vor Codesuche code-suche/Graphify. Alte Belege, Kommandos, Flags, Quellbindungen und Baselineursachen gezielt lesen. Keine Geheimnisse oder ENV-Dateien. Bei fremden/unbekannten Änderungen stoppen.
2. Frisches origin/main holen, festen SHA auflösen. Originalpatch sichern. Eigenen Patch regulär konfliktfrei auf diese Basis abgleichen. Vollständigen tatsächlichen Diff, range-diff und Bytegleichheit des ursprünglichen Patches belegen. A01, B02 und schon integrierte Vorgänger erhalten. Kein Zurücknehmen fremder Änderungen, keine manuelle Konfliktauflösung oder Codekorrektur. Bei notwendiger Änderung Blocker statt Eigenfix.
3. Tatsächlichen finalen Quellstand und Prüfumfang belegen. Bestehende Tests, Formatierung und Clippy gemäß Auftrag, vollständige Crate-Suite mit --no-fail-fast. Flags und optionale Skips ehrlich benennen. Identische bereits geprüfte Basisbäume dürfen mit präziser Kommandobindung wiederverwendet werden. Als erste aktuelle Baseline stehen A01-INTEGRATIONSVORBEREITUNG.md und /tmp/tb-a01-integration.qw1biK/ bereit. Nach jedem Vorgänger dienen dessen tatsächlich gebundene finale Nachweise als mögliche neue Baseline, nicht als ungeprüfter Freibrief. Keine bloße Fehlerzahlgleichheit, kein Erfolg aus Dateialter. Rote Baselines bleiben rot.
4. Sol-only-Gate für exaktes aktuelles Paar, --model gpt-6.1-sol --effort high --timeout 1080. Basis/Head und Nichtleerdiff belegen, passende Vorfahrbeziehung prüfen. Vorhandene Hooks, Gatezustände und Modelle unverändert. Kein --chain, Fallback, Umgebungsbypass oder Ersatztransport. Eigene tatsächlich vorhandene message.model-Felder vor Main-Push als Sol belegen; endgültige Transcriptabnahme erfolgt zusätzlich durch Astra.
5. Wenn Patch unverändert, nötige Prüfungen gebunden und Gate ALLOW: normaler Main-Push im eigenen Worktree mit git push origin HEAD:main. Originalauftrag autorisiert diesen Schritt, keine zusätzliche Plan- oder Nutzerfreigabe nötig. Ein Git-Schritt je Bash-Aufruf, literale absolute Pfade. Keine Shellvariablen im Git-/Gatepfad. Keine Hookablehnung umgehen. Bei menschlicher Permission-Ablehnung oder Hook-BLOCK stoppen, Ursache zurückgeben, nicht über anderes Werkzeug oder anderen Agenten ausführen.
6. Tatsächlichen Push-Exit und Remote-SHA prüfen. Ein bloßer Pushstart ist keine Integration. Commit muss auf Remote-main nachgewiesen sein. Ergebnisse und Grenzen dokumentieren, kein Deploy, Neustart oder Aufräumen. Branch und Worktree erhalten. Keine alten Urteile für einen veränderten Patch verwenden.

Bei fortgeschrittenem Remote-main regulär neu binden, keine Force-Pushes. Keine fachlichen Korrekturen im Integrationskontext. Bei Gate-BLOCK ist ein frischer Fixer außerhalb dieser Rolle erforderlich. Alte ALLOWs, Defaultmodelle oder erneutes Würfeln ersetzen ihn nicht.

## Grenzen und Artefakte

Je Rolle eine neue Datei `${PAKET}-INTEGRATION-02.md` im Taskordner, eigenes neues /tmp-Belegverzeichnis, eigene Code-/Baselineworktrees. Keine anderen Taskdateien verändern. Vorgängernachweis darf gelesen werden. Keine zusätzliche Delegation, Threads, ListAgents oder SendMessage.

Rust 1.97.1, SQLX_OFFLINE=1, cargo-slot, --jobs 1. Keine fremden Prozesse oder Slots verändern; normale eigene Kompilierung nicht pauschal abbrechen. Keine produktiven DB-Schreibbefehle, Migrationen, echten Konten, Secrets/ENV, ai-coach, Python-Anwendungsänderung oder Browserarbeit. Brave verboten. Kein Releasebau, Deploy-Wrapper, Dienstneustart oder Löschen. Der offene Deploykonflikt bleibt unverändert bestehen.

Rückgabe mit fester Basis, Head, unverändertem Patch, vollständigen tatsächlichen Prüfungen, bekannten Lücken, Sol-Gate, Main-Push und Remote-Nachweis. Getrennte Felder für fachliches Urteil, Tests, Integration und fehlende Livewirkung. Bei Blockade erhaltene Arbeit und eigene aktive Aufgaben genau benennen.
