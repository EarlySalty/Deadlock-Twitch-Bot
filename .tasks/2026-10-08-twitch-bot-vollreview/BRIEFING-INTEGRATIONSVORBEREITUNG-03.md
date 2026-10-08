# Integrationsvorbereitung 3: B01, B04, B06, B07 und B10

Die fünf erhaltenen Fixes besitzen echte fachliche Kritiken ALLOW. Modellfelder, Transcript-Hashes und Originalrückgaben der jüngsten Prüfungen sind in ABSCHLUESSE-10.json durch Astra geprüft. Jede Rolle erhält genau ein Paket. Kein neuer Anwendungscodeauftrag, kein Main-Push, Deploy, Neustart oder Aufräumen. Astra übernimmt die tatsächliche Integration nach Abnahme.

## Paketgrenzen

| Paket | Worktree unter /home/nathanael/.worktrees/ | Erhaltener Head | Eigener Patch unter rust/crates/ |
|---|---|---|---|
| B01 | tb-vollreview-idempotenz | c3aa3cc94fec60aa365dcc9ca9f54b4bd3f1539a | tb-internal-api/src/handlers/streamers.rs |
| B04 | tb-vollreview-router-vertraege | d760300d8100b8bf283fee2888c758e6934f3bad | tb-dashboard-api/src/lib.rs |
| B06 | tb-vollreview-proxy-antwort | 46c52a928b47444b10364d024a632ca00226d6c4 | tb-dashboard-api/src/proxy.rs |
| B07 | tb-vollreview-plan-fixture | 9ec607b31a68d2721f6c8905d4268fd5d4290ca6 | tb-dashboard-api/tests/plan_stufen_gates.rs |
| B10 | tb-vollreview-authstatus | 9bce62f1daf52006bb543edc6192769f5971b364 | tb-dashboard-api/src/handlers/auth_status.rs |

Ursprüngliche Basis: a8b5b5e986a1de0b8e2f981651f83bda9cf400dd. B02 ist inzwischen als e98b7f016dbab373a5a8dd9490d158b136c97fec integriert. Frisches origin/main holen, nicht die inzwischen historische Aussage einzelner Rückgaben übernehmen, a8b5b5e9 sei noch aktuell. Der Tracking-Zeiger wird geteilt; festes Basis-/Headpaar und Vorfahrbeziehung vor und nach jeder Prüfung dokumentieren.

## Arbeitsweg

1. AUFTRAG.md und die Originalbelege des eigenen Pakets lesen. Bestehende eigene Prüfprozesse feststellen, keinen Doppelstart erzeugen. Für Quellfragen zuerst code-suche/Graphify, anschließend gezielte Quelle.
2. Originalpatch und Quellhashes sichern. Sauberen eigenen Fix konfliktfrei auf frisches origin/main abgleichen. Nichtleeren tatsächlichen Gesamtdiff, range-diff und exakte Patchgleichheit belegen. Bei Konflikt, neuer Regression oder fachlich notwendiger Quelländerung stoppen und konkrete Abhängigkeit melden. Keine Codekorrektur, keine Nebenbereinigung.
3. Nachweise auf den tatsächlichen finalen Quellbaum binden: Kommando, Setup, Flags, Quellstand vor/nach Ausführung, Exitcode und Loghash. Identische nachweislich geprüfte Quellbäume dürfen wiederverwendet werden. Der integrierte OBS-Fix ist keine bloße Dokumentationsänderung. Fehlende Prüfungen regulär abschließen, passende Baseline mit Ursachenvergleich. Kein Erfolg aus bloßen Fehlerzahlen, Lognamen oder Dateialter.
4. Bestehende Tests des betroffenen Crates vollständig erfassen. Für vollständige Paketsuiten --no-fail-fast verwenden; --include-ignored und --test-threads=1 explizit dokumentieren. Datenbanktests nur synthetisch gemäß Auftrag. Ein Test ohne ausgeführten DB-Pfad ist kein DB-Beweis. Rote Baseline bleibt rot, ein früher Paketabbruch ersetzt keine vollständige Suite.
5. Passender lokaler Sol-Gate: python3 /home/nathanael/Documents/.claude/gpt-workers/gate_hook.py --review --repo <eigener literaler Worktree> --base origin/main --head HEAD --model gpt-6.1-sol --effort high --timeout 1080. Kein --chain, Fallback oder Schutzbypass. Gatezustand nicht ändern. Ein ALLOW gegen eine Nichtvorfahrbasis oder einen Leerdiff reicht nicht.

Die vorhandene fachliche Kritik wird bei exakt erhaltenem Patch nicht wiederholt. Jede notwendige Patchänderung braucht später einen frischen Fixer und eine neue Kritik. Keine solche Rolle selbst starten.

## Spezifische Nachweislücken und vorhandene Belege

B01: B01-PRUEFBINDUNG.md schließt die ursprüngliche Bindungslücke. Neue Regression 1 bestanden/359 gefiltert, neun Loghashes durch Astra geprüft. Historischer tatsächlicher Fixhead 307c09e64528699364036780eb9f6623192be1d8 und aktueller Head haben Rust-Baum 67c189f417131aacbb77fa899d861e91bf5e8ce8; historische Baseline bd695027 und aktuelle Basis haben da48b906659042b04e9166f184868bd85e72bb2e. Historische vollständige Suite 336/24 gegen 335/24, gleiche Fehlernamen. 34 Formatabweichungen unverändert. Jüngstes Clippy scheitert im unveränderten tb-chat/src/scam_pitch.rs:1502. Ein leerer älterer Baseline-Clippylog oder eine andere historische Rust-Basis ist kein vollständiger Baseline-Clippy-Nachweis. Diesen Punkt belastbar schließen oder präzise offenlassen. Logs /tmp/tb-b01-pruefbindung-vFV7lm8L/. Frühere fachliche Kritik in ABSCHLUESSE-07.json bleibt gültig.

B04: Echte neue Kritik ALLOW für d760300d, keine Leerdiff-Abnahme. Diffhash c6bec73321afa74d3235e84068b38042b82e1a4cd23dc83a7347933f0bd5c4c4. Gebundene vollständige Suite 1336 bestanden/35 rot gegen 1333/35; neun Routerfälle einschließlich drei Regressionen bestanden. Astra prüfte reale Zusammenfassungen und Hashes: Fix cc693aba2fb45617dbe4a98a7d41ea6d667aa5ebd022dd6ed19657442955fe1b, Basis 41eaa4d760344d48c680272cf80815b3c9322af88c61724c0940fb76949ac30e. Logs /tmp/tb-b04-router-pruefungen/ und /tmp/tb-b04-pruefabschluss-20261008.dliASa/. Clippy beidseitig Exit 0 mit gleichen Warnungen. Paketformat 265 gleiche Abweichungen. Linkregression belegt das passierte Admin-Gate bis auth_unavailable, keine echte Linkausstellung; Kündigungsregression arbeitet unangemeldet, keine echte Kündigung.

B06: Echte Kritik ALLOW für 46c52a92. Erhaltener Patchhash 43b42f2f4d7a9a520c5629db352c333e6cab1f2307b53ccd1f5548b648e51fa6. Vollständige Suite 1336/35 gegen 1333/35, 14 Proxytests bestanden. Astra prüfte reale Logs und Hashes: Fix ec5cdc2e09337f74d17b0d4fee657e446c417ad7f0e50ecd3fe7b6c6d7d27d82, Basis 097101c4d92bbc0dbd43b5715bd2969033ac9ccdc032780a619598fadcb15c7e. Logs /tmp/b06-pruefabschluss-tTvlYY/. Clippy beidseitig Exit 0. Die eigene Datei ist formatiert; zehn alte Proxy-Formatblöcke verschwinden, 255 fremde Blöcke bleiben unverändert. Nicht pauschal 265 verbleibende Blöcke behaupten. Erster Suiteversuch mit Wrapper-Exit 70 wurde regulär mit Exit 101 abgeschlossen; vorhandenen Wrapper nicht ändern.

B07: Echte Kritik ALLOW für 9ec607b3. Fokusziel plan_stufen_gates: 11 bestanden/1 rot gegen 3/9. Acht vorher rote Fälle bestehen; alle drei vorher grünen bleiben grün. Ein bisher vom Identitätsguard verdeckter TikTok-Eingabefehler bleibt: bestehende Testinputs ohne tiktok_options erreichen jetzt den bestehenden HTTP-400-Guard. Keine neue Regression aus dieser anderen Fehlermeldung ableiten und keinen Zusatzfix vornehmen. Bisher ist lediglich dieses Integrationstestziel geprüft, keine vollständige Crate-Suite. Logs /tmp/tb-b07-plan-fix.log (74bedc99ef3e20ddee55da37bbab14645562b8c1ed37e440eb6d85688ac4dfa3), /tmp/tb-b07-plan-baseline.log (025df2672ef15a9d4e945dc898376cc3d7a2db177d09d289780b3883f7fb63dd), /tmp/tb-b07-pruefabschluss-bhh6gq/. Clippy beidseitig Exit 0, 17 gleiche Warnungsblöcke; Paketformat 265 gleiche Abweichungen.

B10: Echte Kritik ALLOW für 9bce62f1. Astra prüfte tatsächliche Kommandos und Logs: Der Paketaufruf enthielt weder --no-fail-fast noch --include-ignored und brach nach dem Bibliotheksziel ab. 1321 bestanden/21 rot/3 ignoriert gegen 1317/21/3 ist deshalb keine vollständige Crate-Suite. Integrationstests und Doctests fehlen in diesen Logs. 13 Authstatus-Fokustests bestanden. Logs /tmp/tb-b10-pruefung.O7HqMk/, Fix-Bibliothekslog a6b40720ce89f6e98bc7ca099f2722ad48e6366f8126b5d112e0aeefe3bee9bb, Baseline 1b233453e7c1f9ca447ab13c5fa5764e3df02fd0642e74d46940156e20edbb60, Fokus ded33e7d2182e2d9068fb1cb0b25cce4667fd42523672502613f6a98a6088b08. Bestehender Clippy-Abhängigkeitsfehler mit -D warnings. HTTP-Cachepolitik bleibt ausdrücklich C-gesperrt. Rückwärtssprünge der Systemzeit sind nicht der behobene Überholungsfehler.

## Grenzen und Ausgabe

Je Rolle eigene saubere Worktrees, eine Task-Schreibdatei `${PAKET}-INTEGRATIONSVORBEREITUNG.md`, eigenes neues /tmp-Prüfverzeichnis. Keine anderen Taskdateien verändern. Kein Anwendungscode editieren, keine zusätzlichen Commits außer dem erlaubten konfliktfreien Basisabgleich, keine Veröffentlichung. Eigene Branches und Worktrees erhalten. Ein Git-Schritt je Bash-Aufruf, literale absolute Pfade. Rust 1.97.1, SQLX_OFFLINE=1, cargo-slot, --jobs 1. Normale Kompilierung auslaufen lassen; fremde Slots, Prozesse und Dienste nicht verändern.

Keine Secrets/ENV lesen oder ausgeben, keine schreibenden Prod-DB-Befehle, Migrationen, echten Konten, ai-coach oder Browserarbeit. Kein ListAgents, SendMessage oder weiterer T3-Thread. Browserfreigabe besteht für diese Rollen nicht, Brave bleibt verboten. Sol-Modellpflicht, kein Fallback.

Rückgabe: Basis und Head, sauberer Zustand, exakte Patchgleichheit, einzeln gebundene Prüfungen, Baselineursachen, Gatebelege, eigene Hintergrundaufgaben und echte offene Lücken. Teilprüfungen bleiben als solche benannt.
