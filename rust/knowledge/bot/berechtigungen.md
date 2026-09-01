---
title: Berechtigungen und Moderator-Rolle
namespace: bot
category: sicherheit
audience: streamer
last_updated: 2026-09-01
source: code-audit
tip_eligible: false
---
Die Moderator-Rolle des Bot-Accounts ist kein pauschaler Zugriff auf den Twitch-Account des Streamers. Sie erlaubt dem Bot nur konkrete Aktionen im Chat: Ankündigungen senden, erkannte Werbe-Bots löschen oder sperren sowie Sperren wieder aufheben.

Auto-Raids und Clips hängen nicht an der Moderator-Rolle. Dafür autorisiert der Streamer beim Verbinden getrennte, zweckgebundene Twitch-Berechtigungen wie Raid-Verwaltung und Clip-Erstellung. Optionale Funktionen wie die Lurker-Auswertung können zusätzlich Leserechte für die Chatter-Liste benötigen.

Twitch zeigt die angeforderten Berechtigungen im Verbindungsdialog an. Die Verbindung lässt sich jederzeit im Dashboard oder in den Twitch-Einstellungen widerrufen. Ohne die benötigte Moderator-Rolle funktionieren die entsprechenden Chat-Aktionen nicht; davon unabhängige Funktionen erhalten dadurch keine zusätzlichen Rechte.
