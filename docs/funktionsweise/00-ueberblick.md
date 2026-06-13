# Überblick — Was der Bot ist und tut

## Worum es geht

Dieser Bot ist das Werkzeug eines **Partner-Netzwerks für deutsche Deadlock-Streamer**. Sein Leitgedanke ist Gegenseitigkeit: Wenn ein Stream endet, sollen die Zuschauer nicht ins Leere laufen, sondern automatisch zum nächsten passenden Deadlock-Streamer weitergeleitet werden — jeder Teilnehmer schickt jedem anderen Publikum zu. Rund um dieses Raid-Netzwerk bündelt der Bot Schutz (automatische Moderation), Sichtbarkeit (Go-Live-Ankündigungen, Clips), Einblick (Analyse-Dashboard) und Community-Funktionen.

Zielgruppe sind vor allem **kleinere und mittlere Streamer**. Der Bot verspricht keine Zuschauer-Zahlen, sondern hilft den Teilnehmern, sich gegenseitig zu stützen und ihren Kanal besser zu verstehen.

## Was der Bot kann (die Säulen)

- **Auto-Raid-Netzwerk** — leitet am Stream-Ende automatisch zum nächsten passenden, gerade live spielenden Deadlock-Partner weiter; manuelle Raids bleiben möglich. → [raids.md](raids.md)
- **Go-Live-Ankündigungen** — erkennt, wenn ein Partner mit Deadlock live geht, und postet eine anpassbare Discord-Ankündigung; bei Stream-Ende wird daraus eine Offline-/VOD-Ansicht. → [go-live-und-ankuendigungen.md](go-live-und-ankuendigungen.md)
- **Automatische Chat-Moderation** — schützt jeden Partner-Kanal durchgehend vor Werbe- und Viewer-Bots, reagiert auf Chat-Befehle und postet Community-Hinweise. → [chat-moderation-und-befehle.md](chat-moderation-und-befehle.md)
- **Spam- & Bot-Erkennung** — entfernt Spam und Fake-Accounts, bewusst konservativ, damit echte Zuschauer praktisch nie getroffen werden. → [spam-und-bot-erkennung.md](spam-und-bot-erkennung.md)
- **Netzwerkweiter Störer-Schutz** — bekannte Störer werden über alle Partner-Kanäle hinweg ferngehalten. → [schutzlisten-und-global-ban.md](schutzlisten-und-global-ban.md)
- **Scam-Warnungen** — warnt vorsichtig vor gefälschten „offiziellen" Servern und verdächtigen Fremd-Angeboten im Chat. → [scam-warnung.md](scam-warnung.md)
- **AI-Chat-Engagement** — eine ruhige Plauder-KI, die sich an laufende Gespräche andockt (aktuell im beobachtenden Shadow-Modus). → [chat-ai-engagement.md](chat-ai-engagement.md)
- **Community-Funktionen** — Streamer-Leaderboard, Partner-Recruiting und Voice-Reaction. → [community-features.md](community-features.md)
- **Highlight-Clipper & Social-Media** — schneidet automatisch gute Spielmomente und macht daraus nach Freigabe Kurzvideos für TikTok/Instagram/YouTube. → [highlight-clipper.md](highlight-clipper.md), [social-media-uploads.md](social-media-uploads.md)
- **KI-Titelgenerator** — schlägt Stream-Titel auf Basis der eigenen Historie vor. → [titel-generator.md](titel-generator.md)
- **Analyse-Dashboard** — übersetzt die gesammelten Stream-Daten in Kennzahlen, Diagramme und konkrete Coaching-Empfehlungen. → [analytics-und-dashboard.md](analytics-und-dashboard.md)
- **Pläne & Abrechnung** — kostenlose Basis plus optionale bezahlte Zusatz-Features und ein Empfehlungs-Programm. → [plaene-und-billing.md](plaene-und-billing.md)

## Wie ein Streamer einsteigt

Anmeldung läuft über die öffentliche Website per Twitch-Login — kein extra Konto, kein Formular, in der Regel unter einer Minute. Nach Bestätigung der Berechtigungen landet der Streamer direkt im Dashboard, und der Bot beginnt (sobald er als Partner verifiziert ist) mit der Betreuung des Kanals. Die kostenlose Basis (Auto-Raids, volle Moderation, Go-Live-Posts, Chat-Befehle) ist ohne Abo nutzbar. → [website-und-onboarding.md](website-und-onboarding.md)

## So tickt der Bot (Leitprinzipien)

Diese Haltung zieht sich durch alle Funktionen und hilft, sein Verhalten einzuordnen:

- **Gegenseitigkeit statt Einbahnstraße.** Das Netzwerk lebt davon, dass jeder jeden unterstützt — Raids, Recruiting und Community-Funktionen zielen auf gegenseitige Reichweite, nicht auf einseitige Werbung.
- **Ruhig statt Hype.** Wo der Bot schreibt (Promos, Plauder-KI, Warnungen), bleibt der Ton zurückhaltend und menschlich, nie marktschreierisch.
- **Andocken, nicht aufschlagen.** Die Chat-KI eröffnet nie ein Thema aus dem Nichts, sondern knüpft an Bestehendes an.
- **Konservativ moderieren.** Lieber einen Spam-Bot durchlassen als einen echten Zuschauer zu Unrecht bannen; Fehlgriffe sind jederzeit korrigierbar.
- **Ehrlich und transparent.** Der Bot gibt sich klar als Bot zu erkennen, erfindet keine Spielfakten und macht proaktiv sichtbar, was er tut (z. B. öffentlicher Ban-Feed, „Frag den Bot"-Box).
- **Keine Viewer-Versprechen.** Solange Deadlock eine kleine Szene ist, verspricht der Bot keine Zuschauerzahlen — er hilft beim gegenseitigen Stützen und beim Verstehen des eigenen Kanals.

## Was bewusst allgemein bleibt

Einige Funktionen schützen vor Missbrauch oder treffen automatische Auswahl-Entscheidungen — etwa die Spam-Erkennung, die netzwerkweite Bannliste, die Raid-Zielauswahl, die Scam-Erkennung und die Highlight-Auswahl des Clippers. Für diese Bereiche beschreibt diese Doku bewusst nur, **was** der Bot tut und **warum**, nicht die genaue Mechanik (Signale, Schwellen, Gewichtungen, Reihenfolge). Das ist kein Versehen, sondern Absicht: Würde man die exakte Funktionsweise offenlegen, ließe sie sich gezielt umgehen — was den Schutz für alle Kanäle schwächen würde. Wenn jemand nach genauen Werten oder Formeln fragt, lautet die ehrliche Antwort: das ist bewusst nicht offengelegt.

## Schnellnavigation

| Kapitel | Inhalt |
|---------|--------|
| [raids.md](raids.md) | Automatische & manuelle Raids, Schutz vor Raids auf gesperrte Kanäle |
| [go-live-und-ankuendigungen.md](go-live-und-ankuendigungen.md) | Live-Erkennung, Discord-Go-Live-/Offline-Ankündigungen, Vorlagen |
| [chat-moderation-und-befehle.md](chat-moderation-und-befehle.md) | Auto-Moderation, Promos, Lurker-Steuer, vollständige Befehlsliste |
| [spam-und-bot-erkennung.md](spam-und-bot-erkennung.md) | Schutz vor Spam und Fake-/Viewer-Bots |
| [schutzlisten-und-global-ban.md](schutzlisten-und-global-ban.md) | Netzwerkweite Bannliste & Schutzlisten |
| [scam-warnung.md](scam-warnung.md) | Warnungen vor Fake-Servern und Scam-Pitches |
| [chat-ai-engagement.md](chat-ai-engagement.md) | Plauder-KI im Chat (Shadow-Modus) |
| [community-features.md](community-features.md) | Leaderboard, Partner-Recruiting, Voice-Reaction |
| [highlight-clipper.md](highlight-clipper.md) | Automatische Highlight-Clips aus Deadlock-Matches |
| [social-media-uploads.md](social-media-uploads.md) | Clips zu TikTok/Instagram/YouTube, Freigabe-Flow |
| [titel-generator.md](titel-generator.md) | KI-Vorschläge für Stream-Titel |
| [stream-coaching-audit.md](stream-coaching-audit.md) | Internes Sprach-/Coaching-Audit (Slur-Prüfung) |
| [analytics-und-dashboard.md](analytics-und-dashboard.md) | Statistiken, Coaching, Streamer-Dashboard |
| [plaene-und-billing.md](plaene-und-billing.md) | Plan-Stufen, Features, Abrechnung, Affiliate |
| [website-und-onboarding.md](website-und-onboarding.md) | Öffentliche Website, Anmeldung, „Frag den Bot" |
| [admin-funktionen.md](admin-funktionen.md) | Betreiber-Dashboard: Partner-, Billing- und Konfig-Verwaltung |
