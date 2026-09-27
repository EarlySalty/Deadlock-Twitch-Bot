# Eigenprüfung

Umsetzung und Prüfung direkt in dieser Sitzung; kein lokaler Bau-, Review- oder Fix-Agent wurde beauftragt.

Geprüfte Grenzen: stabile Twitch-ID aus der Session; interner Token allein ohne persönliche Sitzung abgewiesen; aktueller aktiver Partner in der DB erforderlich; unbekannte Parameter abgewiesen; private/no-store; Schreibpfade ausschließlich im Bot-Worker.

Geprüfte Vergabe: eindeutige Quellen-IDs, widersprüchliche Wiederholungen abgewiesen, konkurrierende Wochenlimits in Postgres serialisiert, keine Punkte aus Zuschauerzahlen, keine Empfehlungscredits aus administrative added_by-Feldern. Gleichzeitige Steam-Präsenz reicht nicht. Shared-Chat-Ausfälle, fehlende Teilnehmer und große Lücken setzen den Zeitbeleg zurück.

Geprüfte Dauerhaftigkeit: Trigger verhindern Änderung, Löschung und TRUNCATE am Ledger. Nullpunkt-Teilnahmen oberhalb des Limits bleiben nachvollziehbar. Erreichte Achievement-Stufen bleiben gespeichert. Wochenzeitbelege bleiben nach Rohdatenretention erhalten. Berlin-Kalendergrenzen und DST sind getestet.

Geprüfte Integration: tatsächlicher Clip-Outbox-Vertrag mit metadata und submission_id; Streamer-ID aus der bestehenden Einreichung; qualifizierte Join-ID und separate Referral-Credits aus dem Nachbarprojekt; keine zweite Clip-, Invite- oder OAuth-Implementierung. Das bestehende Ledger-Schema bleibt für PR 995/996 lesbar.

Betriebsgrenze: Die bestehenden Quelltabellen aus den parallelen Invite- und Clip-Contest-Arbeiten sowie der Zentraldatenbank-Lesezugang müssen beim späteren Rollout verfügbar sein. Ohne diese Voraussetzungen liefert die API 503. Es wurde nichts produktiv migriert oder gestartet. Die Quellen-Integration wurde mit isoliertem Postgres und simuliertem Helix geprüft, nicht durch Veränderungen an echten Community-Konten.

Der PR darf in diesem Auftrag nicht gemergt oder deployt werden.
