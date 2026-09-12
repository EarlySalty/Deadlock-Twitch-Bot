---
title: "Uplink: OBS einrichten"
namespace: bot
category: setup
audience: streamer
last_updated: 2026-09-12
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

### Schritt 5: Twitch-VOD-Spur bei Benutzerdefiniert freischalten

Wenn **Twitch-VOD-Spur** unter Einstellungen → Ausgabe → Erweitert → Stream bereits sichtbar ist, ist keine Dateiänderung nötig. Setze dann direkt den normalen Audiotrack auf **1** und Twitch-VOD-Spur auf **2**.

Fehlt das Feld, OBS vollständig beenden – auch im Infobereich/Tray – und `user.ini` öffnen. Unter Windows: **Win + R** drücken, `%APPDATA%\obs-studio` eingeben, Enter drücken und `user.ini` mit einem Texteditor öffnen. Standardpfade:

- Windows: `%APPDATA%\obs-studio\user.ini`
- macOS: `~/Library/Application Support/obs-studio/user.ini`
- Linux: `~/.config/obs-studio/user.ini`
- Flatpak: `~/.var/app/com.obsproject.Studio/config/obs-studio/user.ini`

Bei Portable-OBS liegt die Datei im verwendeten lokalen OBS-Konfigurationsordner. Im **bereits vorhandenen** Abschnitt `[General]` genau die Zeile `EnableCustomServerVodTrack=true` ergänzen. Keinen zweiten `[General]`-Abschnitt anlegen. Datei speichern und OBS neu starten. Ein Streamprofil-Import setzt diese globale Einstellung nicht.

Danach unter Einstellungen → Ausgabe → Erweitert → Stream den normalen Audiotrack auf **1** und **Twitch-VOD-Spur** auf **2** setzen. Die Audioquellen in den erweiterten Audioeigenschaften den beiden Mischungen zuordnen. Musik, die nicht ins VOD soll, bleibt zum Beispiel nur auf Spur 1. Nicht dieselbe Spur für Live und VOD auswählen.

Der Dienst Benutzerdefiniert bleibt absichtlich erhalten, damit OBS den AV1-Eingang an Uplink senden kann. Der native Twitch-Dienst ist im geprüften OBS-Dienstprofil auf H.264 beschränkt. Uplink übernimmt danach die Twitch-Ausgabe und ordnet die beiden Audiomischungen serverseitig den Twitch-Rollen Live und VOD zu.

### Schritt 6: Fenster einrichten

Verbinde deine Plattformen in ihren Dashboard-Karten. Gespeicherte Wunschprofile werden anhand des Eingangs und des Kanalzugangs geprüft; das tatsächlich aktive Profil wird getrennt angezeigt. Profiländerungen gelten zunächst für den nächsten Stream. Die privaten, dauerhaft gespeicherten Dockadressen findest du im selben Dashboard. Danach startest du in OBS wie gewohnt.
