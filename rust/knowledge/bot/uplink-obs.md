---
title: "Uplink: OBS einrichten"
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

Für Twitch verwendet Uplink immer zwei getrennte OBS-Mischungen: **OBS-Spur 1 ist der Livestream, OBS-Spur 2 ist das Twitch-VOD.** Dafür gibt es im Dashboard keine Audiowahl mehr. Uplink prüft beim Streamstart, ob beide Spuren wirklich ankommen. Fehlt Spur 2, wird nur der Twitch-Ausgang angehalten; der Live-Mix wird niemals still als VOD-Ersatz kopiert.

Beim Dienst Benutzerdefiniert ist die native VOD-Auswahl in OBS standardmäßig verborgen. In dessen `user.ini` muss im vorhandenen Abschnitt `[General]` die Zeile `EnableCustomServerVodTrack=true` stehen. OBS dafür vollständig beenden, nur diese Einstellung ergänzen und neu starten. Ein Streamprofil-Import setzt diesen globalen Wert nicht. Ältere OBS-Stände können eine andere Konfigurationsaufteilung verwenden.

Danach unter Einstellungen → Ausgabe → Erweitert → Stream den normalen Audiotrack auf **1** setzen, die VOD-Tonspur aktivieren und als VOD-Track **2** wählen. Die Audioquellen in den erweiterten Audioeigenschaften den beiden Mischungen zuordnen. Musik, die nicht ins VOD soll, bleibt zum Beispiel nur auf Spur 1. Die Bezeichnung kann weiterhin Twitch-VOD-Spur lauten. Ist die Auswahl nicht vorhanden, bleibt die Zweispur-Einrichtung offen; nicht dieselbe Spur zweimal auswählen.

Der Dienst Benutzerdefiniert bleibt absichtlich erhalten, damit OBS den AV1-Eingang an Uplink senden kann. Der native Twitch-Dienst ist im geprüften OBS-Dienstprofil auf H.264 beschränkt. Uplink übernimmt danach die Twitch-Ausgabe und ordnet die beiden Audiomischungen serverseitig den Twitch-Rollen Live und VOD zu.

### Schritt 5: Fenster einrichten

Verbinde deine Plattformen in ihren Dashboard-Karten. Gespeicherte Wunschprofile werden anhand des Eingangs und des Kanalzugangs geprüft; das tatsächlich aktive Profil wird getrennt angezeigt. Profiländerungen gelten zunächst für den nächsten Stream. Die privaten, dauerhaft gespeicherten Dockadressen findest du im selben Dashboard. Danach startest du in OBS wie gewohnt.
