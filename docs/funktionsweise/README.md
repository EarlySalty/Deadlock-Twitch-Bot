# Funktionsweise — Agent-Wissensbasis

Diese Sammlung beschreibt den Twitch-Bot **rein funktional**: was er tut, wann er es tut und was Streamer, Zuschauer und Admins davon sehen oder einstellen können. Sie ist als geschlossene **Wissensbasis für einen Frage-Antwort-Agenten** gedacht — ein Support-/Erklär-Agent, der damit jede Nutzerfrage zum Bot beantworten kann.

Einstieg: **[00-ueberblick.md](00-ueberblick.md)**.

## Für den Agenten: wie diese Doku zu benutzen ist

- **Nur aus diesen Dateien antworten.** Diese Sammlung ist die einzige Quelle. Was hier nicht steht, weiß der Agent nicht — er soll es nicht erfinden.
- **Funktional, nicht technisch.** Die Doku enthält bewusst keine technischen Interna (Ports, Datenbanktabellen, interne Endpunkte, Datei-/Funktionsnamen, eingesetzte Programmiersprachen oder Bibliotheken). Fragen danach sind nicht Gegenstand dieser Wissensbasis.
- **Geheimnis-Grenze respektieren.** Für die unten genannten Bereiche ist die genaue Mechanik bewusst nicht dokumentiert. Fragt jemand nach exakten Schwellen, Signalen, Gewichtungen, Formeln oder der Auswahl-/Prüf-Reihenfolge, ist die ehrliche und vollständige Antwort: *„Das ist bewusst nicht offengelegt, damit sich der Schutz/die Auswahl nicht gezielt umgehen lässt."* — nicht raten, nicht spekulieren.
- **Ton wie der Bot selbst:** sachlich, ehrlich, ruhig. Kein Marketing-Geschwurbel, keine Viewer-Versprechen.

## Kapitel-Index

| Datei | Thema |
|-------|-------|
| [00-ueberblick.md](00-ueberblick.md) | Einstieg: was der Bot ist, seine Säulen, Leitprinzipien, Schnellnavigation |
| [raids.md](raids.md) | Raids (automatisch & manuell), Schutz vor Raids auf gesperrte Kanäle |
| [go-live-und-ankuendigungen.md](go-live-und-ankuendigungen.md) | Live-Erkennung, Go-Live-/Offline-Ankündigungen, Vorlagen & Platzhalter |
| [chat-moderation-und-befehle.md](chat-moderation-und-befehle.md) | Auto-Moderation, Promos, Lurker-Steuer, vollständige Befehlsliste |
| [spam-und-bot-erkennung.md](spam-und-bot-erkennung.md) | Schutz vor Spam und Fake-/Viewer-Bots |
| [schutzlisten-und-global-ban.md](schutzlisten-und-global-ban.md) | Netzwerkweite Bannliste & Schutzlisten |
| [scam-warnung.md](scam-warnung.md) | Warnungen vor Fake-Servern und Scam-Pitches |
| [chat-ai-engagement.md](chat-ai-engagement.md) | Plauder-KI im Chat (aktuell Shadow-Modus) |
| [community-features.md](community-features.md) | Leaderboard, Partner-Recruiting, Voice-Reaction |
| [highlight-clipper.md](highlight-clipper.md) | Automatische Highlight-Clips aus Deadlock-Matches |
| [social-media-uploads.md](social-media-uploads.md) | Clips zu TikTok/Instagram/YouTube, Freigabe-Flow |
| [titel-generator.md](titel-generator.md) | KI-Vorschläge für Stream-Titel |
| [stream-coaching-audit.md](stream-coaching-audit.md) | Internes Sprach-/Coaching-Audit |
| [analytics-und-dashboard.md](analytics-und-dashboard.md) | Statistiken, Coaching, Streamer-Dashboard |
| [plaene-und-billing.md](plaene-und-billing.md) | Plan-Stufen, Features, Abrechnung, Affiliate |
| [website-und-onboarding.md](website-und-onboarding.md) | Öffentliche Website, Anmeldung, „Frag den Bot" |
| [admin-funktionen.md](admin-funktionen.md) | Betreiber-Dashboard: Partner-, Billing- und Konfig-Verwaltung |

## Pflege-Regeln (für Menschen, die hier editieren)

Diese Wissensbasis ist absichtlich von der technischen Entwickler-Doku getrennt. Damit sie sicher hinter einem Agenten bleiben kann, gelten beim Ergänzen/Ändern feste Regeln:

**Niemals aufnehmen — Betriebsgeheimnisse.** Für diese Bereiche nur *was/warum*, nie die Mechanik:

- Spam-/Bot-Erkennung (Signale, Muster, Schwellen, Verschleierungs-Abwehr, Lernverfahren)
- Netzwerkweite Bannliste & Schutzlisten (Aufnahme-Kriterien, Taktung, Fristen)
- Raid-Score & Raid-Zielauswahl (Bewertungs-Faktoren, Gewichtung, Reihenfolge)
- Partner-Recruiting-Auswahl (Eignungs-Kriterien, Schwellen)
- Scam-Erkennung (Erkennungssignale, Muster)
- AI-Engagement (Auslöse-Bedingungen, Persona-/Grounding-Interna)
- Highlight-Clipper (Erkennungs-Merkmale, Grenzwerte)
- Generell: jede Schwelle, jedes Gewicht, jede Signal-Liste, jede Prüf-Reihenfolge und jede Formel, die Missbrauchs-Resistenz oder eine automatische Auswahl ausmacht.

**Niemals aufnehmen — technische Interna.** Portnummern, Datenbanktabellen/-spalten, interne API-Endpunkte, Datei-/Funktions-/Klassen-/Variablennamen, Secret-Namen, Programmiersprachen, Frameworks/Bibliotheken, Prozess-/Service-Aufteilung.

**Sprache & Stil.** Deutsch mit echten Umlauten (ä/ö/ü/ß). Pro Kapitel das Format aus den bestehenden Dateien beibehalten: *Worum es geht · Was der Bot tut · Wann es passiert · Was Streamer/Viewer sehen · Was Streamer einstellen können · Grenzen & Sonderfälle · Häufige Fragen.*

**Faustregel beim Schreiben:** Wenn ein Satz einem Angreifer helfen würde, den Schutz oder die Auswahl zu umgehen, gehört er nicht hierher — beschreibe das Ergebnis, nicht den Weg dorthin.

> Hinweis: Die funktionsgenaue technische Entwickler-Doku liegt getrennt unter `docs/architecture/` und `rust/docs/`. Diese enthält bewusst Details (auch Mechaniken) und ist **nicht** für den öffentlich befragbaren Agenten bestimmt.
