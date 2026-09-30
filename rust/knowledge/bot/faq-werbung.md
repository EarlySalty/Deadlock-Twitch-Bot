---
title: Chat-Werbung des Bots
namespace: bot
category: faq
audience: streamer
last_updated: 2026-09-18
source: manual
tip_eligible: false
---
Was der Bot in deinen Chat schickt, wann er das tut und wie du Werbung abschaltest.

### Welche Werbung schickt der Bot in meinen Chat?

Die Community-Werbung dreht sich nur um den Community-Discord und fünf belegte Themen: Mitspieler mit Voice-Lanes, komplett kostenloses Coaching, deutsche Deadlock-Patchnotes, Turniere und Scrims sowie Community-Events. Der Bot soll dabei auf das reagieren, was im Chat gerade passiert, statt einen Standardsatz nach dem Muster "wer X sucht, im Discord gibt es Y" abzuspulen.

Periodische Ansagen bekommen den Discord-Link automatisch angehängt. Persönliche Anlassantworten tragen keinen direkten Link. Builds, Items, Meta-Beratung und andere Leistungen sind keine Werbethemen mehr.

- Ein Satz statt Werbeblock.
- Bezug auf Titel oder Chat, wenn ein passender Aufhänger da ist.
- Pro Nachricht höchstens ein Community-Nutzen.
- Keine Mitgliederzahlen, Superlative oder erfundenen Termine.
- Den periodischen Text kannst du im Dashboard durch einen eigenen Text ersetzen.

[Dashboard öffnen](https://deutsche-deadlock-community.de/twitch/auth/login?next=%2Ftwitch%2Fdashboard-v2)

### Wen spricht der Bot persönlich auf die Community an?

Die bestehende Anlasslogik prüft zuerst die normalen Gates für Partnerkanal, Deadlock-Stream, Zuschauerstatus, Cooldowns und Nachrichteninhalt. Sie wird durch diese Änderung nicht neu in die Chat-Pipeline eingehängt. Wenn der Pfad bereits läuft, darf die Antwort nur eines der fünf erlaubten Community-Themen nennen und muss natürlich zur Nachricht passen. Bei Unsicherheit bleibt der Bot still.

Der Bot sagt einem Zuschauer nicht, dass er als neu erkannt oder getrackt wurde. Kanalbetreiber, Moderatoren, Bots und bereits ausgeschlossene Empfänger bleiben von der persönlichen Ansprache ausgenommen.

### Werden andere Deadlock-Streamer im Chat als Partner angeworben?

Der kalte Partner-Pitch bleibt deaktiviert. Nur weil jemand in einem Partnerkanal schreibt oder selbst Deadlock streamt, startet dadurch keine neue Partnerwerbung.

Neu ist eine eng begrenzte Ausnahme nach einem echten Raid: Raidet ein Deadlock-Streamer einen aktiven Partnerkanal, kann der Bot sich für den Raid bedanken und den Raider auf die Streamer-Gemeinde hinweisen. Dafür müssen alle folgenden Bedingungen erfüllt sein:

- Der Zielkanal ist aktiver Partner und streamt in diesem Moment Deadlock.
- Der Raider ist anhand seiner Twitch-User-ID weder Partner noch ehemaliger Partner.
- Für den Raider gibt es eine Deadlock-Session aus den letzten 60 Tagen.
- Der Raider steht nicht auf der Blacklist.
- Es gibt keinen früheren Outreach-Eintrag und keinen bereits geposteten Streamer-Pitch im Ledger.
- Der Zielkanal hat Werbung nicht über Plan oder Werbefrei-Schalter deaktiviert.

Der Raid-Dank ist ein fester Text ohne Modellaufruf. Er verlinkt auf die Streamer-Seite unter https://deutsche-deadlock-community.de/streamer. Nach dem ersten Ledger-Eintrag wird derselbe Raider nicht erneut über diesen Weg angesprochen. Wenn eine notwendige Datenbankprüfung fehlschlägt, wird der Raid-Dank nicht gesendet.

### Wann wird periodische Werbung gepostet?

Die periodische Einladung braucht die konfigurierten Aktivitäts- und Zeitgrenzen. Der Textgenerator bekommt den aktuellen Titel und jüngere Chatnachrichten als Kontext. Er darf daraus nur dann einen konkreten Aufhänger machen, wenn er bei einem der fünf erlaubten Community-Themen bleibt.

Bei aktiven Sonder-Events kann der globale Aktions-Text Vorrang haben. Der vorhandene Werbefrei-Gate bleibt dabei bestehen.

### Wie schalte ich die Chat-Werbung ab?

Der Werbefrei-Plan und Bundles mit derselben Berechtigung setzen das Promo-Gate für den Kanal. Das gilt auch für den neuen Raid-Dank. Zusätzlich bleibt der bestehende harte Schalter in streamer_plans.promo_disabled erhalten.

[Pläne ansehen](https://deutsche-deadlock-community.de/twitch/auth/login?next=%2Ftwitch%2Fabbo)

### Kann ich nur den periodischen Werbe-Text anpassen?

Ja. Im Dashboard kannst du den Text der periodischen Einladung ersetzen. Der Platzhalter {invite} wird beim Senden durch den Discord-Link ersetzt. Ein aktiver globaler Sonder-Text kann zeitweise Vorrang haben.

[Dashboard öffnen](https://deutsche-deadlock-community.de/twitch/auth/login?next=%2Ftwitch%2Fdashboard-v2)
