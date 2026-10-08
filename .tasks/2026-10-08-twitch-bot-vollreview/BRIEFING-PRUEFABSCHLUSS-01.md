# Prüfabschluss A02, B05 und B02

BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 20 min als Orientierung, keine Kompilierungs-Abbruchfrist | Worktree: je Paket unten

## Ausgangslage und Auftrag

Frischer Sol-Worker je Paket. Die vorherigen Fixer sind beendet. Dieser Auftrag vervollständigt Prüfungen, Basisbindung und Sol-Gate. Anwendungscode nicht ändern. Falls ein konkreter Compiler-, Test- oder Gatefehler im eigenen Diff Änderungen erfordert, mit Beleg zurückgeben; dann folgt ein frischer Fixer. Fremde Bestandsfehler nicht reparieren. Keine parallelen Writer auf den genannten Paketdateien.

A02 und B05 hatten ihren vorbereiteten Diff uncommittet zurückgelassen. Die automatisch folgenden Kritiker sahen deswegen lediglich Basis gleich Head. Ihre BLOCK-Urteile belegen den unveränderten Ausgangsfehler, bewerten aber den tatsächlichen Arbeitsdiff nicht. Astra hat die bereits vorbereiteten Sol-Dateien ohne Quelländerung lokal als WIP-Commits gesichert. Das ist weder fachliche Abnahme noch Mergefreigabe. Der ursprüngliche Nutzerauftrag umfasst ausdrücklich Fix, Merge und Push; diese private Sicherung ändert keine Deploygrenze.

## Pakete

### A02

- Worktree `/home/nathanael/.worktrees/tb-vollreview-affiliate-eigentuemer`, Branch `fix/vollreview-affiliate-eigentuemer`.
- Lokaler Prüfcheckpoint `864e70f6`, Basis `bd69502728861f8279e8b044592ecac9058fc732`. Eigenen vollen SHA selbst feststellen. Ausschließlich handlers/affiliate.rs und handlers/affiliate_portal.rs geändert, 286 Einfügungen und 38 Löschungen.
- Grundvertrag: BRIEFING-W02-FIXGRUPPE-02.md, Abschnitt A02. Bisher keine Kompilierung oder Testbaseline, keine wirksame Gateprüfung. Eigene Datei-rustfmt und diff --check waren erfolgreich.
- Eigener Baseline-Worktree `tb-vollreview-affiliate-eigentuemer-baseline` auf bd695027. Vorhandenen Zustand prüfen. Keine tatsächlichen Tests aus dem leeren Log `/tmp/tb-a02-baseline-test.log` ableiten.
- Erstfixer ac41b95dd02c44824: 92 Sol-Datensätze, SHA256 7f7a30749f03a0cb8e5faca10b5ffb7fe4a1846d2ab0120a87d4d04cc79ae66e. Leerdiff-Kritiker a7c52e52be738fc09: 38 Sol-Datensätze, SHA256 5efd3f1afb3937c754f77079b1f0677f8fbe0ba63a0a1991af656222af6440ef. Von Astra geprüft.

### B05

- Worktree `/home/nathanael/.worktrees/tb-vollreview-plattform-refresh`, Branch `fix/vollreview-plattform-refresh`.
- Lokaler Prüfcheckpoint `131a45ab`, Basis bd695027. Ausschließlich handlers/platform_token.rs und handlers/plattform_oauth.rs geändert, 218 Einfügungen und 22 Löschungen. platform_store.rs bleibt als bisher erlaubter Paketpfad unverändert.
- Grundvertrag: BRIEFING-W02-FIXGRUPPE-02.md, Abschnitt B05. Keine Kompilierung oder Testbaseline. Frühere Ausgabe ALLOW: no reviewable changes galt dem leeren Commitvergleich und ist ausdrücklich keine Freigabe dieses Fixes.
- Eigener Baseline-Worktree `tb-vollreview-plattform-refresh-baseline`. Vier bisherige Cargo-Logs blieben ohne Slot leer. Nicht als ausgeführte Prüfungen zählen.
- Erstfixer a0bb997f18fef04b5: 101 Sol-Datensätze, SHA256 ea6717c8a48c578229d0d106ca0c0fb8fc797bb5f3647480b1fd9812af58c815. Leerdiff-Kritiker a09287615bfbad1e8: 28 Sol-Datensätze, SHA256 3cad859e755c7b7908db4832e906d1fe76386c9ffd31cba53dddd221ab5d6a6f. Von Astra geprüft.
- Ein gemeldeter Ablaufwert-Zwilling in plattform_connect.rs ist ein separater ungeprüfter Claim. Weder ändern noch als zusätzlichen Fixauftrag behandeln.

### B02

- Worktree `/home/nathanael/.worktrees/tb-vollreview-obs-start`, Branch `fix/vollreview-obs-start`.
- Fester Head `e98b7f016dbab373a5a8dd9490d158b136c97fec`, Basis `a8b5b5e986a1de0b8e2f981651f83bda9cf400dd`. Genau obs/bus.rs und obs/ws.rs geändert. Grundvertrag BRIEFING-B02.md.
- Gate ALLOW und frische fachliche Kritik ALLOW für diesen festen Diff. Fixer a8b8882199a6cd853: 251 Sol-Datensätze, SHA256 a5326db8175884f665d818fb2451336ee5af78065bbaa031ded50aa7d88ed586. Kritiker aef3c467a1da3ca9d: 36 Sol-Datensätze, SHA256 2065d1ef5b067d026e3ef8aefc6fe7f2c1ca7b8a1db1fdd3c7067962fbb789d9. Von Astra geprüft.
- 50 OBS-Tests samt echtem LISTEN/NOTIFY bestanden vor dem letzten Basisabgleich; vollständige damalige Suite 1334 bestanden und 35 identische Baselinefehler. Am endgültigen SHA fehlen Testlauf und Fix-Clippy. Paket-fmt hat 265 bekannte Bestandsabweichungen. Keine grüne Gesamtsuite behaupten.
- Eigener Baseline-Worktree `tb-vollreview-obs-start-baseline`. Bestehende Logs und genaue ursprüngliche Rückgaben in wf_586b3f73-0dc/journal.jsonl gezielt lesen, nicht vollständige Transcripts in den Kontext laden.

## Vorgehen

1. Eigenen Worktreezustand und tatsächliche SHAs prüfen. Aktuelles origin/main holen und eigene unveröffentlichte Commits bei Bedarf abgleichen. Rebase ändert keinen fachlichen Scope. Bei Konflikt oder konkreter nötiger Quelländerung mit Beleg abgeben, nicht improvisieren. Jeder Git-Schritt einzeln mit literalen absoluten Pfaden.
2. Fehlende Prüfungen am tatsächlichen Fixstand abschließen: eigene Datei-rustfmt, Paket-fmt, Clippy und bestehende tb-dashboard-api-Tests. Erst günstige paketbezogene Regressionen, dann bestehende Suite. Toolchain `RUSTUP_TOOLCHAIN=1.97.1-x86_64-unknown-linux-gnu`, `SQLX_OFFLINE=1`, `/home/nathanael/.local/bin/cargo-slot`, Kompilierung `--jobs 1`. Synthetische Testdatenbank `TB_TEST_DATABASE_URL='postgres:///tb_bb_test?host=/var/run/postgresql'`, `TB_TEST_REQUIRE_DB=1`. Normale Kompilierung angemessen lange als verfolgte Hintergrundaufgabe laufen lassen; kein voreiliges Beenden nach einigen Minuten. Keine fremden Slots, Prozesse oder Dienste verändern.
3. Bestandsfehler gegen die passende unveränderte Basis abgrenzen. Bereits vorhandene eigene Nachweise nur wiederverwenden, wenn Quellstand, Prüfkommando und Voraussetzungen exakt belegt übereinstimmen; Dateialter ist kein Beweis. Unterschiedliche Testflags oder Rust-Bäume nicht gleichsetzen. Fehlende Ergebnisse ausdrücklich melden. Testdatenbankdateien nur auf Existenz prüfen, keine Zugangsdaten lesen.
4. Abschließender Sol-Gate auf tatsächlichem Fix-Commit: `python3 /home/nathanael/Documents/.claude/gpt-workers/gate_hook.py --review --repo <eigener literaler Worktree> --base origin/main --head HEAD --model gpt-6.1-sol --effort high --timeout 1080`. Leerer Diff ist kein Fixnachweis. Bei BLOCK abgeben; keine Codekorrektur im selben Kontext. Kein --chain, Modellrückfall oder Hook-Bypass.

Kein Merge nach main, Release, Deploy oder Restart. Keine Anwendungscodeänderung, zusätzlichen Agenten/Threads/ListAgents/SendMessage, Nutzerfragen, Secrets/ENV-Dateien, Produktionsdatenbank, Browser, Kontenaktionen oder ai-coach. Vor Codesuche Graphify. Taskdokumente gehören Astra. Ausschließlich gpt-6.1-sol.

Rückgabe strukturiert: tatsächlicher Basis-/Head-SHA, sauberer Worktree ja/nein, Änderungen seit Prüfcheckpoint ja/nein, Prüfungen/Exit-Codes, Baseline, Gate und Belege, laufende eigene Hintergrundaufgaben, Blocker. Bei A02/B05 folgt ein neuer Kritiker für den echten Diff. B02 erhält keine unnötige Wiederholung derselben Kritik, sofern der geprüfte fachliche Diff unverändert nachgewiesen ist.
