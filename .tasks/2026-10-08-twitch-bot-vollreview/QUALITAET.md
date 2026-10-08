# Bauqualität des Twitch-Bots

Stand: 2026-10-08. Codebasis: `0ecae1370f1a80d1a101249b5c932663d69be8af`.

Ein Bereich von 108 ist bewertet und unabhängig gegengeprüft. Eine Gesamtnote und eine bereichsübergreifende Rangfolge wären derzeit unbelegt. Alle Empfehlungen sind Klasse C und werden in diesem Auftrag nicht umgesetzt.

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

## Noch offen

107 Bereichsbewertungen samt unabhängigen Qualitätskritiken; danach Gesamteinschätzung und die fünf Empfehlungen mit dem größten bereichsübergreifenden Nutzen.
