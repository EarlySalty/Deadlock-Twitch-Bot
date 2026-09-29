# Eigenprüfung

Umsetzung und Prüfung direkt in dieser Sitzung; kein lokaler Bau-, Review- oder Fix-Agent wurde beauftragt.

Geprüfte Grenzen: stabile Twitch-ID aus der Session; interner Token allein ohne persönliche Sitzung abgewiesen; aktueller aktiver Partner in der DB erforderlich; unbekannte Parameter abgewiesen; private/no-store; Schreibpfade ausschließlich im Bot-Worker.

Geprüfte Vergabe: eindeutige Quellen-IDs, widersprüchliche Wiederholungen abgewiesen, konkurrierende Wochenlimits in Postgres serialisiert, keine Punkte aus Zuschauerzahlen, keine Empfehlungscredits aus administrative added_by-Feldern. Gleichzeitige Steam-Präsenz reicht nicht. Shared-Chat-Ausfälle, fehlende Teilnehmer und große Lücken setzen den Zeitbeleg zurück.

Geprüfte Dauerhaftigkeit: Trigger verhindern Änderung, Löschung und TRUNCATE am Ledger. Nullpunkt-Teilnahmen oberhalb des Limits bleiben nachvollziehbar. Erreichte Achievement-Stufen bleiben gespeichert. Wochenzeitbelege bleiben nach Rohdatenretention erhalten. Berlin-Kalendergrenzen und DST sind getestet.

Geprüfte Integration: tatsächlicher Clip-Outbox-Vertrag mit metadata und submission_id; Streamer-ID aus der bestehenden Einreichung; qualifizierte Join-ID und separate Referral-Credits aus dem Nachbarprojekt; keine zweite Clip-, Invite- oder OAuth-Implementierung. Das bestehende Ledger-Schema bleibt für PR 995/996 lesbar.

Betriebsgrenze: Die bestehenden Quelltabellen aus den parallelen Invite- und Clip-Contest-Arbeiten sowie der Zentraldatenbank-Lesezugang müssen beim späteren Rollout verfügbar sein. Ohne diese Voraussetzungen liefert die API 503. Es wurde nichts produktiv migriert oder gestartet. Die Quellen-Integration wurde mit isoliertem Postgres und simuliertem Helix geprüft, nicht durch Veränderungen an echten Community-Konten.

Der PR darf in diesem Auftrag nicht gemergt oder deployt werden.

## Fix-Runde vom 29. September 2026 zu Review 7d82cb75

1. `sources/cursor.rs`: Referral-Credits werden aus `twitch_streamer_referral_credits` im Twitch-Pool gelesen. Quell-ID, Empfangsbeleg und numerisch sortierter Text-Cursor folgen dem echten Primärschlüssel; die Test-Migration ist bytegleich mit #999.
2. `sources/cursor.rs`: Qualifizierte Joins stammen aus `bot.twitch_invite_joins` im Zentraldatenbank-Pool, mit `join_id`, Guild-ID, qualifiziertem Zustand und gespeicherter Partner-Twitch-ID. Der Integrationstest verwendet Tabelle und Zustands-Trigger aus Deadlock-Bots #466.
3. `quests.rs`: Ein erreichbarer Zweierpool wird vergeben und abgerechnet; 503 bleibt auf einen tatsächlich leeren Aufgabenpool beschränkt.
4. `sources/live.rs`: Bestätigte Discord-/Steam-Links zählen als Mitspieler ohne Twitch-Profil. Bekannte ausgetretene Streamer und deaktivierte Zuordnungen bleiben ausgeschlossen.
5. `sources/live.rs`: GC-Aufträge mit `steam_id`, `steam_id64` oder `account_id` werden gegen dieselbe verifizierte Steam-ID geprüft. Widersprüchliche Felder im Auftrag oder Ergebnis werden abgewiesen.
6. `projection.rs`: Eine alleinige Stream-Verlängerungsaufgabe liefert keinen unabhängigen Streak-Beleg. Der Integrationstest prüft anschließend eine echte separate Einladung.
7. `store.rs`, `projection.rs`: `credited_at` bestimmt den Berlin-Monat, `occurred_at` bewahrt den Quellzeitpunkt. Das Oktober/November-Beispiel ist getestet. Der Monatsabschluss- und Raid-Vertrag für #996 steht in `docs/partner-effort-engine.md`; #996 muss Filter, Gleichstand und gemeinsamen Advisory-Lock übernehmen.

Die eigenständige Review-Runde und das Merge bleiben nach der vorgegebenen Fix-Runde bei einem anderen Thread.
