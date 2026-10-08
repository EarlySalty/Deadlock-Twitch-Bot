# Bauqualität des Twitch-Bots

Stand: 2026-10-08. Codebasis: `0ecae1370f1a80d1a101249b5c932663d69be8af`.

Zwölf Bereiche von 108 sind bewertet und unabhängig gegengeprüft. Eine Gesamtnote und eine bereichsübergreifende Rangfolge wären derzeit unbelegt. Alle Empfehlungen sind Klasse C und werden in diesem Auftrag nicht umgesetzt. Qualitätsnoten ersetzen keine Sicherheits- oder Fixfreigabe. Die neun W03-Qualitätsbewertungen sind abgeschlossen; ihre Defektreviews laufen getrennt weiter.

Skala: 1 mangelhaft, 2 schwach, 3 solide mit deutlichen Grenzen, 4 gut, 5 sehr gut.

## R09: gemeinsamer HTTP-Kern

Note: **3 von 5**, vom frischen Sol-Kritiker bestätigt. Alle acht Primärdateien mit 680 Zeilen wurden gelesen.

| Aspekt | Belegtes Urteil | Fundstellen am Review-SHA |
|---|---|---|
| Struktur | Kleine Module mit getrennten Zuständigkeiten; Auth-Zustand wird ausdrücklich übergeben. Keine Datenbank-, Prozess- oder ENV-Aufrufe im Primärcode. | `rust/crates/tb-http-core/src/lib.rs:3-11`, `src/middleware/auth.rs:16-18,76-80` |
| Doppelte Pfade | Drei nahezu gleiche Bytevergleiche. Die darüberliegenden Sicherheitsregeln unterscheiden sich bewusst und dürfen nicht pauschal vereinheitlicht werden. | `rust/crates/tb-http-core/src/middleware/auth.rs:118-128`, `rust/crates/tb-internal-api/src/security.rs:172-178,208-216`, `rust/crates/tb-crypto/src/token.rs:56-64` |
| Fehlerzustand | `ApiError` bietet eine gemeinsame HTTP-Ausgabe, führt aber statische und dynamische Payloads parallel. Beide Ausgabewege wählen gegenwärtig konsistent. | `rust/crates/tb-http-core/src/error.rs:20-24,205-224` |
| Testlage | 15 deklarierte Primärtests, ohne Datenbank ausführbar. Ein Raid-Handler-Test verwendet andere Guards als der produktive Router. Spezialisierte Guard-Tests existieren bereits. | `rust/crates/tb-http-core/src/lib.rs:59-115`, `src/middleware/auth.rs:169-247`, `rust/crates/tb-internal-api/src/handlers/raid.rs:98-108`, `src/lib.rs:414-423`, `src/security.rs:632-726` |
| Wartbarkeit | Keine Riesendateien im Kern. Generische Guards und `IdempotencyKey` haben im untersuchten Rust-Bestand keine produktiven Aufrufer; einzelne Integrationskommentare sind veraltet. | `rust/crates/tb-http-core/src/middleware/idempotency.rs:15-24`, `rust/crates/tb-internal-api/src/lib.rs:55,418` |

### Empfehlungen, ausschließlich Klasse C

1. Testfixtures und produktive Guard-Kette gemeinsam aufbauen. Nutzen: Handler-Tests prüfen die tatsächlich vorgeschaltete Schutzkette. Risiko: niedrig bis mittel; bestehende Guard-Tests und produktive Regeln erhalten.
2. Bytevergleich aus `tb-crypto` wiederverwenden, sofern die zusätzliche Crate-Kopplung vertretbar ist. Nutzen: eine gepflegte Vergleichsprimitive. Risiko: mittel; Trimming, Leerwertbehandlung, Origin-Prüfung und Session-Brücke getrennt lassen.
3. Effektive `ApiError`-Payload künftig kapseln und Parität von `payload_json` und `IntoResponse` absichern. Nutzen: weniger redundante Zustände. Risiko: mittel, weil öffentliche Felder und Antwortformen Verträge sind.
4. Nutzung generischer Guards und des Idempotenz-Extraktors dokumentieren. Nutzen: wirksame Schutzketten sind erkennbar. Risiko: niedrig für Dokumentation, mittel für spätere API-Änderungen. Keine ungeprüfte Löschung.

### Grenzen und Nachweise

Testcode wurde gelesen, aber keine Tests, Builds oder Live-Prüfungen ausgeführt. Referenzpakete wurden nur entlang der Integration untersucht. Keine Aussage zu externen Crate-Nutzern oder zur installierten Proxy-Konfiguration. `ai-coach` blieb ausgeschlossen.

- Reviewer: `a88b3416857fc4d40`, Workflow `wf_b0f0e2fe-347`; Sol-Nachweis in `MODELLE.md`.
- Frischer Kritiker: `a1ea9dd26f72a7c58`, Workflow `wf_37ee8d9d-602`; Urteil `BESTÄTIGT`, keine Korrekturen.
- Kritiker-Transcript: 35 Modellnachrichten, ausschließlich `gpt-6.1-sol`; SHA256 `913af586bec5c2d3174e01129ac4a8b2e8bef5819fab2d0a62c48c63a803e966`.
- Die Kritiker-Fundstellen wurden per `git show 0ecae137…:<pfad>` gelesen. Der abweichende Hauptcheckout floss damit nicht in die Bewertung ein.
- Rohbewertungen stehen in den jeweiligen nativen `journal.jsonl`-Dateien unter der Session `f61905e7-f7ff-405b-a6d7-090dec371fcb/subagents/workflows/`.

## DA01: API-Router, Zustand und Proxy

Note: **3 von 5**. Der frische Kritiker bestätigt die Note, korrigiert aber pauschale Aussagen zu Ressourcen, Struktur und Konfigurationsmigration.

| Aspekt | Geprüftes Urteil | Fundstellen am Review-SHA |
|---|---|---|
| Zusammensetzung | Fachliche Subrouter und getrennte öffentliche API-/Seitenrouter existieren. Globale Abhängigkeiten, Integrationsaufbau und Hintergrundarbeit konzentrieren sich dennoch im Routeraufbau. Die lange Routentabelle allein belegt keine schlechte Schichtung. | `rust/crates/tb-dashboard-api/src/lib.rs:156,451,1287,2157-2173,2276` |
| Zustandsmodell | Analyse-Reservierung ist transaktional; Abschluss ist wiederholbar. Cleanup besitzt einen Eigentümer und Abbruchtests. Der separate Plattform-Refresh behält dagegen keinen Task-Handle. | `rust/crates/tb-dashboard-api/src/ai_store.rs:51,76,185,238,328,429`; `rust/crates/tb-dashboard-api/src/lib.rs:2276` |
| Proxygrenzen | Die sichtbare 16-MiB-Grenze begrenzt den Request, nicht die gepufferte Upstream-Antwort. Dashboard- und interner Proxy haben unterschiedliche Host- und Zugriffsverträge. Keine pauschale Zusammenlegung. | `rust/crates/tb-dashboard-api/src/proxy.rs:73,99,147,202`; `rust/crates/tb-internal-api/src/handlers/legacy_proxy.rs:31` |
| Testlage | Produktionsbuilder-Wiring-Tests und AiStore-Lebenszyklustests existieren. Nicht belegt sind im untersuchten Bestand die vollständige main-Komposition mit getrennten Rollen sowie Refresh-Shutdown und Audit-Persistierungsfehler. | `rust/crates/tb-dashboard-api/src/lib.rs:2452,2460`; `rust/crates/tb-dashboard-api/src/admin_audit.rs:119,197` |
| Konfiguration | Uplink hat einen bewusst begrenzten neuen Startvertrag. Benachbarte ENV-Leser belegen unterschiedliche Quellen, aber keine gescheiterte Uplink-Migration. Optionaler Python-Fallback hat einen Startup-Aufrufer; Live-Nutzung bleibt ungeprüft. | `rust/crates/tb-dashboard-api/src/uplink_config.rs:13,105,180`; `rust/bin/tb-dashboard/src/main.rs:381,497` |

C-Empfehlungen: Zusammensetzung und Hintergrundarbeit bei einem gesonderten Umbau trennen; vorhandenes `TaskSupervisor`-Muster berücksichtigen. Test-DB-Infrastruktur unter Erhalt der Rollenverträge vereinheitlichen. Konfigurationsquellen und Fallback-Verbraucher zunächst inventarisieren, keine ungeprüfte Abschaltung oder API-Zusammenlegung. Der Nutzen liegt in reproduzierbaren Integrationstests und klarer Task-Verantwortung; Eingriffe in Bootstrap und Sicherheitslayer haben mittleres Risiko.

Gezielte statische Lektüre, keine vollständige Neubewertung sämtlicher Fachhandler. Keine ausgeführten Tests oder Live-Nachweise. Reviewer `a817daeec624cc91f`, Kritiker `a8a7d2ae5acb95b6b`, Workflow `wf_67858741-57f`. Urteil `KORRIGIERT`; die obige Fassung übernimmt die Korrekturen. Modellnachweise in MODELLE-W02.md.

## DA02: Sitzungen, Identität, CSRF und Rechte

Note: **3 von 5**, vom frischen Kritiker bestätigt. Explizite Rechte- und Identitätsmodelle sowie vorhandene Regressionstests stehen einer stark gekoppelten Sitzungsverwaltung gegenüber.

| Aspekt | Geprüftes Urteil | Fundstellen am Review-SHA |
|---|---|---|
| Identität | Admin, Partner und unangemeldet sind eigene Zustände. Owner-Zuordnung verwendet eine positive Plattform-ID statt wiederverwendbarer Logins; gemeinsame Ownership-Auflösung existiert. | `rust/crates/tb-dashboard-api/src/auth/level.rs:50-90,293-317,685-720`; `rust/crates/tb-dashboard-api/src/auth/streamer_scope.rs:136-174` |
| Speicherung | Hashing, Verschlüsselung und atomarer Einmalverbrauch sind gemeinsam vorhanden. Sitzungsvarianten werden überwiegend als manuell interpretiertes JSON gespeichert; einzelne Speicherabläufe wiederholen sich. Unterschiedliche Identitäts- und Ablaufregeln bleiben fachlich getrennt. | `rust/crates/tb-dashboard-api/src/auth/session.rs:605-740,968-998,1036-1102,1767-1907,2061-2119` |
| Kopplung | Die 3872 Zeilen von `session.rs` enthalten Tests, bündeln aber auch produktiv Cookies, Caches, Persistenz, Fingerprints und mehrere Anmeldewege. `PlayerSession` ist bereits eine teilweise typisierte Alternative. | `rust/crates/tb-dashboard-api/src/auth/session.rs:432-514,1413-1504`; `rust/crates/tb-dashboard-api/src/auth/session/player.rs:10-24,36-41,56-104` |
| Testlage | 134 Testattribute in elf geprüften Auth-Dateien. Identitätswiederverwendung, Player-Isolation und Kryptomanipulation besitzen Testquellen. Einige DB-Fixtures machen aus Einrichtungsfehlern einen stillen Skip; vorhandene isolierte Fixtures sind strenger. | `rust/crates/tb-dashboard-api/src/auth/security.rs:453-473`; `rust/crates/tb-dashboard-api/src/auth/csrf.rs:469-521`; `rust/crates/tb-dashboard-api/src/handlers/player_connect/tests.rs:212-270`; `rust/crates/tb-dashboard-api/src/test_database.rs:19-88` |
| Fehler und Konfiguration | Kryptografie hat konkrete Fehlertypen. Einige Validierungsgrenzen reduzieren Ergebnisse auf None, bool oder einen gemeinsamen Rückfallzweig. Schlüsselkonfiguration wird an mehreren Stellen als ungeprüfter String eingelesen. | `rust/crates/tb-dashboard-api/src/auth/fernet.rs:41-84`; `rust/crates/tb-dashboard-api/src/auth/level.rs:415-459`; `rust/bin/tb-dashboard/src/main.rs:515-532`; `rust/crates/tb-dashboard-api/src/lib.rs:2165-2172` |

C-Empfehlungen: Test-Voraussetzungen und echte Einrichtungsfehler unterscheidbar machen, vorhandene isolierte PostgreSQL-Fixtures nutzen. Speichermechanik schrittweise von fachlicher Sitzungspolitik trennen und Payloads typisieren. Fehlerunterscheidungen und Konfigurationsübergabe bei einem gesonderten Umbau präzisieren. Eine Route-/Auth-/CSRF-Matrix pflegen; alte Helfer erst nach Prüfung externer Nutzer stilllegen. Erwarteter Nutzen ist besser prüfbares Auth-Verhalten, bei Policy- und Persistenzänderungen besteht hohes Regressionsrisiko. Kein Umbau ist hier freigegeben.

Gezielte Produktionsausschnitte und Stichproben im Testcode, keine vollständige erneute Lektüre aller zugeordneten Quellen durch den Kritiker. Kein Testlauf, keine gemessene Coverage und keine Live-Konfigurationsprüfung. Reviewer `a49d3c978a85ec7f3`, Kritiker `a69cce616175f3ffb`, Workflow `wf_67858741-57f`, Urteil `BESTÄTIGT`. Modellnachweise in MODELLE-W02.md.

## W03: neun weitere Bereiche

Der Workflow `wf_461b0370-4c3` ist abgeschlossen. Alle 18 Modellnachweise sind geprüft; 957 echte Sol-Datensätze, Einzelhashes in MODELLE-W03-QUALITAET.md. Vollständige korrigierte Kurzbewertungen und C-Empfehlungen stehen in QUALITAET-W03.md. Zielgerichtete statische Lektüre, keine grünen Testläufe oder Live-Nachweise.

| Bereich | Note | Kritiker | Begründung und exemplarischer Beleg |
|---|---:|---|---|
| DA03, Anmeldungen | 3/5 | BESTÄTIGT | Injizierbare Anbieter und atomarer State-Verbrauch; konzentrierter Callback und uneinheitliche Testvoraussetzungen. `rust/crates/tb-dashboard-api/src/handlers/auth_login.rs:199-453,1346-1365`. |
| DA04, Admin-Aktionen | 3/5 | BESTÄTIGT | Gemeinsame Fachmutationen und typisierter Betriebseditor; kopierte Dienstkonfiguration und veraltete Schutztexte. `rust/crates/tb-dashboard-api/src/handlers/admin_streamers.rs:525,708,780`. |
| DA05, Diagnose | 3/5 | KORRIGIERT | Gemeinsame Lader und begrenzte SQL-Ausführung; dreifacher Fingerprint-Vertrag. Vorhandene Konfigurations- und Testbausteine anerkannt. `rust/crates/tb-dashboard-api/src/handlers/system/health.rs:57-109`. |
| DA06, Billing/Stripe | 3/5 | BESTÄTIGT | Gemeinsamer Transport und transaktionaler Webhook; widersprüchliche Katalogannahmen und unwirksame einzelne Testzweige. `rust/crates/tb-dashboard-api/src/handlers/billing_stripe_sync.rs:123-125,193-200,335-359`. |
| DA07, Affiliate/Profile | 3/5 | KORRIGIERT | Explizite Sichtbarkeits- und Revisionsregeln; gemischte HTTP-/Persistenzlogik. Vorhandene isolierte Tests und Helix-Testzugänge berücksichtigt. `rust/crates/tb-dashboard-api/src/handlers/partner_profiles.rs:659-675,834-842`. |
| DA15, Formulare/Wettbewerb | 4/5 | BESTÄTIGT | Monatssperre, Outbox, Feedback-Idempotenz und substanzielle DB-Tests; Lücken der vollständigen Wettbewerbs-Handlerkette. `rust/crates/tb-dashboard-api/src/handlers/clip_contest/contest.rs:421-477,605-663`. |
| DA17, Plattformen/Overlays | 3/5 | BESTÄTIGT | Klare OAuth-Testnähte und bestehende Lifecycle-Bausteine; globaler Caster-Hub und dünnere Integrationsprüfung. `rust/crates/tb-dashboard-api/src/handlers/caster_overlay.rs:399,610,968`. |
| MO01, EventSub | 3/5 | BESTÄTIGT | Dauerhafte Inbox und getrennte Annahmeschichten; breite Subscription-Orchestrierung und begrenzte Rückmeldungen einiger Hooks. `rust/crates/tb-monitoring/src/inbox_store/mod.rs:119,214,300`. |
| IA01, interner API-Kern | 3/5 | KORRIGIERT | Explizite Ports und zentrale Guards; doppelte Partner-Persistenz. Vorhandener Startsnapshot und Routertests begrenzen die Kritik. `rust/crates/tb-internal-api/src/streamer_lifecycle.rs:201,256`. |

## Noch offen

96 Bereichsbewertungen samt unabhängigen Qualitätskritiken; danach Gesamteinschätzung und die fünf Empfehlungen mit dem größten bereichsübergreifenden Nutzen.
