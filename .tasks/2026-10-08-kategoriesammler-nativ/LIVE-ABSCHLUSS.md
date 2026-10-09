# Nativer Abschluss am 9. Oktober 2026

## Ergebnis

Produktiver Quellstand: `e0e9fde20ec27f87acc8833e3d93dcdbe4d2934d`. Die Sammlung läuft im Prozess des Twitch-Bots. Die alte Collector-Unit ist nicht mehr vorhanden. Der korrigierte Watchdog liest die gemeinsame Konfiguration und beendet auch nachfolgende Timerläufe erfolgreich. Der Speicher-Nachtrag wurde bis zu diesem Nachweis nicht implementiert; kein Archiv wurde hochgeladen, umgewandelt oder lokal entfernt.

## Veröffentlichung und tatsächliche Aktivierung

Fixrunde 7 wurde mit dem aktuellen main 4177752a integriert. Der lokale Gate des sauberen Abschlusskandidaten meldete: `ALLOW: The timestamp fix preserves exact microsecond matching and existing cutover safeguards. No blocking defects found in the supplied diff.` Der geschützte Push auf main endete mit Exit 0.

Der erste eigene Release-Lauf blieb ohne Compilerstart in der vorgesehenen Warteschlange und wurde vor Ablauf seines zu kurzen Zeitbudgets geordnet gestoppt. Der Ersatzlauf erhielt den regulären Slot und baute sieben Binaries in 20 Minuten und 40 Sekunden, Exit 0. Die Herkunftsmarken entsprechen dem vollständigen sauberen e0e9fde2-SHA. Drei Frontends wurden ebenfalls erfolgreich gebaut.

Der anschließende serialisierte Deploy-Aufruf endete mit Exit 1, weil sein SHA-Buildziel bereits existierte. Dieser Aufruf wird nicht als erfolgreicher eigener Deploy ausgegeben. Eine lesende Prüfung zeigte zu diesem Zeitpunkt bereits e0e9fde2 als aktuellen Release und als Quellstand der drei laufenden Dienste. Die sieben tatsächlich installierten ELF-Marken wurden einzeln geprüft. Fremde Sessions und deren Build- oder Deploylogs wurden nicht kontaktiert oder gelesen.

Danach startete diese Session die drei erlaubten Twitch-Dienste ausdrücklich über den bestehenden Restart-Wrapper neu. Der dokumentierte Kurzname `twitch-bot-watchdog` war im installierten Wrapper nicht zugelassen. Dessen eigene Liste erlaubte den vollständigen Namen `deadlock-twitch-bot-watchdog`; der anschließende Aufruf über diesen Namen endete mit Exit 0. Kein allgemeiner systemctl- oder Hook-Umweg wurde verwendet.

## Prozess- und Inhaltsbeweis

| Dienst | PID unmittelbar vorher | PID nach eigenem Neustart | Start in Berlin |
|---|---:|---:|---|
| Twitch-Bot | 2453730 | 2692127 | 9. Oktober 2026, 04:56:37 CEST |
| Dashboard | 2454071 | 2692369 | 9. Oktober 2026, 04:56:37 CEST |
| Stream-Audit | 2455109 | 2694550 | 9. Oktober 2026, 04:56:43 CEST |

Die lesende Wrapperprüfung bestätigt e0e9fde2 für current und die drei ausführbaren Prozesse, keinen gelöschten exe-Pfad und NRestarts=0. Die beiden Inhaltsanker `Bot-Konfigurationsgruppe fehlt` im Watchdog und `category chat drain timed out` im Bot sind in den tatsächlich installierten Binaries nachgewiesen.

Das root-sichtbare Journal von Bot, Dashboard, Stream-Audit und Watchdog enthielt seit dem eigenen finalen Botstart keine Einträge mit Priorität err. Die gefilterten Bot-Meldungen zeigen die native Lease, den anonymen Lesepfad und minütlich bestätigte Snapshots.

## Datenfluss und Watchdog

Die lesende PostgreSQL-Probe vom 9. Oktober 2026 um 04:57:54 CEST bestätigt Prozess 2692127 und Lease `2692127-1791514597888877`. Prozess-ID und Lease stimmen in Runtime und Sammlerstatus überein. Beide Heartbeats sind frisch, beide Lease-Zustände aktiv, und die exakt zu diesem Status gehörige Messung liegt nach dem tatsächlichen finalen Neustart. Der Vergleich verwendet die gespeicherte Mikrosekundenauflösung.

Seit diesem Start wurden bis zur Probe zwei Messläufe mit 1.332 erwarteten und genau 1.332 gespeicherten Snapshot-Zeilen sowie 70 neue Chatzeilen aus 14 Räumen bestätigt. Platten- und Rohchatpause waren false. Die beiden produktiven Native-Migrationen 20261008160000 und 20261008161000 bleiben erfolgreich und checksumgleich.

Der Watchdog meldet Result=success und ExecMainStatus=0. Zwei nachfolgende Timerinvokationen um 04:57:23 und 04:57:53 CEST haben unterschiedliche InvocationIDs und erfolgreiche Abschlüsse. Der Timer ist aktiv und wartet auf den nächsten Lauf. Die alte Unit meldet LoadState=not-found und ActiveState=inactive.

## Prüfgrenzen und Folgeauftrag

Die vorhandene Wrapper-Suite bestand am integrierten e0e9fde2-Stand 18 Tests ohne ignorierte Fälle, einschließlich der echten synthetischen PostgreSQL-Probe. Der vorherige Lauf ohne Test-DSN mit übersprungener DB-Klasse zählt nicht als Vollnachweis. Fixrunde 7 bestätigt elf passende und 17 abgewiesene Zeit-, Identitäts-, Lease-, Freshness- und Cutoverfälle. Die zuvor vollständig bestandene native Rust-Suite umfasst acht Bibliotheks- und acht Watchdog-Tests; deren Quellen wurden danach nicht geändert.

Dashboard-Ort ist die adminpflichtige Route `/twitch/kategorie`, Navigation „Deadlock weltweit“. Die produktive angemeldete Ansicht wurde nicht geprüft. Frühere lokale Moli-Fixtures sind keine Produktionsanmeldung. Die native Laufzeit und der Watchdog sind dennoch separat live belegt. Eine globale grüne Bot-Clippy- oder Dashboard-Gesamtsuite wird nicht behauptet; die zuvor zahlenmäßig belegten fremden Baselinefehler bleiben abgegrenzt.

Die Originalbelege dieses Abschlusses liegen im Unterordner `belege/nativer-abschluss-e0e9fde2`. Nach gesicherter Akte werden die eigenen nativen Abschlussbranches und Worktrees entfernt. Anschließend beginnt NACHTRAG-1-SPEICHER.md auf einem eigenen direkten Folgebranch. Vor dem ersten lokalen Entfernen ist der fertige Rust-Trockenlauf mit Tageszahlen vorzulegen.

TESTNACHWEIS[TW-1]: 18 passed, 0 ignored | Baseline: 0 rot im Wrapperlauf
MERGEPROTOKOLL[MS-1]: 4 Git-Schritte einzeln | Anläufe: 1 | Gate: ALLOW mit gpt-6.1-sol
LIVEBEWEIS[DV-1]: PID 2453730->2692127 | exe ohne (deleted) | journal -p err leer | Anker "Bot-Konfigurationsgruppe fehlt" in Watchdog-Binary | Funktion: native Messung und erfolgreiche periodische Watchdogläufe | Ort: Adminroute /twitch/kategorie, angemeldete Ansicht offen
TEXTNACHWEIS[DR-1]: Gedankenstriche 0 | ae/oe/ue/ss-Ersatz 0 | Absolutwörter 0 belegt | Senke: Auftragsakte
