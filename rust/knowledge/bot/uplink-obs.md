---
title: Uplink: OBS einrichten
namespace: bot
category: setup
audience: streamer
last_updated: 2026-09-08
source: Uplink-Dashboard
tip_eligible: false
---

### Verbindung aus dem Dashboard übernehmen

Öffne in OBS Einstellungen → Stream und wähle Benutzerdefiniert. Kopiere die öffentliche RTMPS-Serveradresse aus dem Dashboard in Server. Deinen privaten Uplink-Schlüssel kopierst du getrennt in Streamschlüssel. Er gehört weder in die Serveradresse noch in den Stream.

### Ausgabe einstellen

Öffne Einstellungen → Ausgabe und wähle Erweitert. AV1 ist bevorzugt, H.264 wird ebenfalls unterstützt. Wähle einen Encoder, den deine OBS-Version für diesen RTMPS-Dienst anbietet. HEVC verwendest du nur mit einem dafür freigegebenen Eingangsprofil. Uplink prüft die tatsächlich empfangenen Eigenschaften.

### Bitrate und Qualität

Verwende CBR und plane Audio sowie Reserve innerhalb deines gemessenen Uploadbudgets ein. Die gewünschte Bitrate einer Plattform ist keine Vorgabe für deinen Upload. Auflösung, Bewegung und Encoder beeinflussen die Bildqualität; eine feste Einsparung ist nicht garantiert. Als Keyframe-Intervall wählst du 2 s.

### Live- und VOD-Ton

Wenn dein VOD anderen Ton benötigt, bereite Live-Mix und VOD-Mix als getrennte Audiospuren in OBS vor. Eine Stereo-Spur ersetzt keine zwei Mischungen. Ob beide Spuren über deinen OBS-Dienst ankommen, muss der Eingangsstatus bestätigen. Fehlenden erforderlichen VOD-Ton ersetzt Uplink nicht still durch den Live-Mix.

Beim Dienst Benutzerdefiniert ist die zweite Tonspur im geprüften OBS-Quellstand an eine globale Einstellung gebunden. In dessen `user.ini` muss im vorhandenen Abschnitt `[General]` die Zeile `EnableCustomServerVodTrack=true` stehen. OBS dafür vollständig beenden, nur diese Einstellung ergänzen und neu starten. Ein Streamprofil-Import setzt diesen globalen Wert nicht. Ältere OBS-Stände verwendeten eine andere Konfigurationsaufteilung; die Anleitung ist kein Nachweis für jede historische Version.

Danach unter Einstellungen → Ausgabe → Erweitert die VOD-Tonspur aktivieren und für Livestream und VOD unterschiedliche Spurnummern auswählen. Die Audioquellen in den erweiterten Audioeigenschaften den beiden Mischungen zuordnen. Die Bezeichnung kann weiterhin Twitch-VOD-Spur lauten. Ist die Auswahl nicht vorhanden, bleibt die Zweispur-Einrichtung offen; nicht einfach dieselbe Spur zweimal auswählen.

„Twitch“ mit eigener Serveradresse ist kein belegter AV1-Ersatz: Der gewöhnliche Twitch-Dienst schränkt die Codec-Auswahl ein. AV1 setzt einen passenden Encoder im Dienst Benutzerdefiniert voraus. Dieser anhand des Quellcodes erklärte Weg ist noch kein vollständiger OBS-/Twitch-Live- und VOD-Nachweis. Quellen und geprüfter OBS-Commit stehen im technischen Dashboardvertrag; es wird keine lokale Erweiterung vorausgesetzt.

### Schritt 5: Fenster einrichten

Verbinde deine Plattformen in ihren Dashboard-Karten. Gespeicherte Wunschprofile werden anhand des Eingangs und des Kanalzugangs geprüft; das tatsächlich aktive Profil wird getrennt angezeigt. Profiländerungen gelten zunächst für den nächsten Stream. Die privaten, dauerhaft gespeicherten Dockadressen findest du im selben Dashboard. Danach startest du in OBS wie gewohnt.
