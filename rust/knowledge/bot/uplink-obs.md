---
title: Uplink in OBS einrichten
namespace: bot
category: setup
audience: streamer
last_updated: 2026-08-22
source: rs-relay/docs/obs.html
tip_eligible: false
---
Du stellst den Weg zu uns ein. CBR und das Keyframe für Twitch musst du nicht übernehmen, und welche Auflösung rausgeht, wählst du im Dashboard. Ziel ist viel Bild bei wenig Upload und wenig Last neben dem Spiel.

### 1. Ausgabe

Öffne in OBS **Einstellungen**, dann **Ausgabe**. Wähle beim **Ausgabemodus** **Erweitert** und öffne den Reiter **Stream**.

#### Videokodierer

Nimm den ersten Eintrag, den OBS bei dir anbietet:

1. `NVIDIA NVENC HEVC`, `AMD HW H.265 (HEVC)` oder `QuickSync HEVC`
2. Wenn HEVC fehlt oder das Spiel danach ruckelt: `NVIDIA NVENC H.264`, `AMD HW H.264` oder `QuickSync H.264`
3. `x264` ohne Hardware-Encoder. Das kostet Kerne und mehr Upload.

Lass `x265`, `SVT-AV1`, `AOM AV1` und Hardware-AV1 weg. Diese Varianten sind neben dem Spiel zu schwer oder passen nicht zu unserem Eingang. Unser Eingang erwartet HEVC oder H.264.

### Qualitätsregulierung

CBR, VBR, ABR und CQP oder CRF bestimmen, wie viele Bits eine Szene bekommt. Sie ändern nicht den Codec.

| Modus | Was er macht | In Deadlock | Zu uns |
| --- | --- | --- | --- |
| **CBR** | Jede Sekunde bekommt gleich viele Bits, in der Lobby wie im Fight. | Upload wird in ruhigen Szenen verschenkt. Der Fight wird dadurch nicht schöner. | Nicht nötig. Twitch und Kick brauchen CBR von uns, nicht von dir. |
| **VBR** | Zielbitrate plus Maximalbitrate. In ruhigen Szenen spart VBR, im Fight gibt es mehr, begrenzt durch das Maximum. | Planbar und passend für deine Leitung. | Empfehlung. |
| **CQP** oder bei x264 **CRF** | Du setzt eine Qualitätszahl. Der Encoder nimmt so viele Bits, wie die Szene braucht. | In der Lobby oft 2 bis 3 Mbit, im Teamfight plötzlich 10 bis 15 Mbit. | Beste Qualität pro Bit, wenn deine Leitung die Spitze trägt. |
| **ABR** | Hält im Schnitt eine Bitrate, oft ohne hartes Maximum. Das fällt vor allem bei x264 auf. | Kann über deine Leitung schießen, ohne so klar zu sein wie CQP. | Weglassen. Das ist der schlechtere Kompromiss. |

- **VBR**: Du begrenzt die Leitung. Der Encoder verteilt innerhalb der Grenze.
- **CQP oder CRF**: Du bestellst Qualität. Die Bitrate folgt der Szene.
- **ABR**: Ungefähr VBR, aber ohne zuverlässigen Deckel.
- **CBR**: Deckel und Boden sind gleich. Für den direkten Twitch-Weg ist das Pflicht, für uns ist es Verschwendung.

#### Welche Zahl?

**Standard bei knapper Leitung: VBR**

- Zielbitrate: 6000 Kbps
- Maximalbitrate: 8000 Kbps oder 80 Prozent vom gemessenen Upload, falls das niedriger ist
- Unter 5 Mbit realem Upload: Ziel 4000, Maximum 5000 und 30 fps

**CQP oder CRF**, wenn der Upload stabil über 10 Mbit liegt und du das Maximum willst:

- AMD oder Intel: CQP 20. 18 ist schärfer und schwerer, 22 ist sparsamer.
- NVIDIA: CQP 18 bis 20
- x264 als Notnagel: CRF 20

Spiele danach eine echte Runde. Steigt die Bitrate in OBS ständig über deine Leitung, wechsel zurück zu VBR.

Setze das Keyframeintervall auf **2 s**, nicht auf 0 für automatisch.

Lass **Ausgabe umskalieren** deaktiviert, solange die Basis 1080 oder 1440 ist. Runterskalieren in OBS nimmt uns Detail weg.

Als x264-Notnagel nimm das Preset `veryfast`, das Profil `high` und das Tune `zerolatency`. Das Tune `film` ist für Aufnahmen, nicht für Live.

### Audio

- Kodierer: FFmpeg AAC
- Spur 1 einschalten
- 160 oder 192 Kbps, Stereo
- Die Twitch-VOD-Spur kannst du ausschalten. Die Plattform-Spuren werden von uns gebaut.

### 2. Video, Auflösung und FPS

Öffne in OBS **Einstellungen**, dann **Video**.

| Gemessener Upload | Basis und Ausgabe | FPS | Kodierung |
| --- | --- | --- | --- |
| 3 bis 5 Mbit | 1920×1080 | 30 | HEVC, VBR 4000, maximal 5000 |
| 5 bis 8 Mbit | 1920×1080 | 60 | HEVC, VBR 6000, maximal 8000 |
| Ab 8 Mbit, wenn die GPU 1440 ohne Drops hält | 2560×1440 | 60 | HEVC, VBR 6000, maximal 8000 |
| Wenn die GPU Drops hat oder das Spiel ruckelt | Eine Stufe kleiner oder 30 fps |  | Derselbe Encoder, nicht auf Software wechseln |

1440p an uns zu schicken lohnt sich zweifach: entweder rechnen wir daraus 1080p und behalten dabei Schärfe, oder wir senden die vollen 2K weiter, wenn du diese Stufe im Dashboard wählst. Es lohnt sich nicht, wenn der Encoder Frames schluckt. **Skipped frames** bleibt in OBS bei 0.

### 3. Ziel: unser Dienst

Öffne in OBS **Einstellungen**, dann **Stream**.

1. Dienst: **Benutzerdefiniert**
2. Server: die **SRT-Adresse** aus dem Dashboard. Kopiere sie, statt sie abzutippen.
3. Stream-Schlüssel: bleibt leer. Dein Schlüssel steckt bei SRT bereits als `streamid` in der Adresse.

Ins Dashboard kommt nur die SRT-Adresse, und über die läuft dein Stream. SRT verträgt ein Wackelnetz besser. Im Dashboard steht genau eine Adresse, und das ist die SRT-Adresse. Kann dein Programm kein SRT, nimm OBS.

Mit **Stream starten** sendest du danach an uns. Die Plattformen starten wir. Titel und Kategorie stellst du weiter in den Dashboards der Plattformen ein, solange die Synchronisation dafür noch nicht verfügbar ist.

### 4. Chat und die OBS-Fenster zurückholen

Sobald der Dienst auf **Benutzerdefiniert** steht, verschwinden in OBS die Twitch-Fenster: Chat, Aktivitätsfeed und Stream-Informationen. Dein Chat läuft trotzdem normal weiter. Chat und Video sind bei Twitch getrennte Wege, wir fassen nur das Video an. Weg sind nur die Fenster in OBS, weil OBS die ausschließlich bei verbundenem Twitch-Konto einblendet.

Du holst sie in einer Minute zurück:

1. In OBS: **Docks**, dann **Benutzerdefinierte Browser-Docks**.
2. Pro Zeile einen Namen und die Adresse aus der Tabelle eintragen. `DEINKANAL` durch deinen Kanalnamen ersetzen.
3. Fenster anordnen, dann unter **Docks** das Layout speichern.

| Fenster | Adresse |
| --- | --- |
| Chat | `https://www.twitch.tv/popout/DEINKANAL/chat?darkpopout` |
| Aktivitätsfeed | `https://dashboard.twitch.tv/popout/stream-manager/activity-feed` |
| Stream-Informationen | `https://dashboard.twitch.tv/popout/stream-manager/edit-stream-info` |
| Kanalpunkte | `https://dashboard.twitch.tv/popout/stream-manager/community-points` |

Nur beim Chat steht dein Kanalname in der Adresse. Bei den drei anderen schickt dich Twitch automatisch auf deinen eigenen Kanal, sobald du angemeldet bist. Im Dashboard stehen alle Adressen fertig zum Kopieren.

Es sind übrigens dieselben Seiten, die OBS auch in seine eingebauten Twitch-Fenster lädt. Der Unterschied ist nur, dass OBS sie bei verbundenem Konto selbst anlegt. Du legst sie einmal von Hand an, ordnest die Fenster und speicherst unter **Docks** das Layout. Das übersteht jeden OBS-Neustart.

#### „Aber ich bin doch gar nicht angemeldet“

Das ist die häufigste Frage an dieser Stelle, und dahinter stecken zwei verschiedene Anmeldungen:

- **Konto verbinden** in den Stream-Einstellungen. Damit holt sich OBS deinen Twitch-Streamschlüssel. Die brauchst du bei uns nicht, deshalb ist sie weg.
- **Anmeldung im Fenster.** In OBS steckt ein vollständiger Browser mit eigenen Cookies, genau wie Chrome oder Firefox. Damit meldest du dich bei Twitch an, und darüber laufen Chat, Aktivität und Stream-Info.

Beim ersten Öffnen zeigt das Fenster einen Anmelden-Knopf. Ein Klick, einmal einloggen, fertig. OBS legt das Cookie ab und findet es beim nächsten Start wieder.

Die eingebauten Twitch-Fenster machen es genauso: dieselben Seiten, derselbe Browser, dasselbe Cookie. Du sparst dir mit ihnen nur das Anlegen, nicht die Anmeldung.

Über unseren Server läuft davon nichts. Dafür müssten wir deine Twitch-Sitzung durch unsere Hand laufen lassen, und wer bei uns einbräche, hätte damit Zugriff auf deinen Kanal. Deine Anmeldung bleibt zwischen dir und Twitch.

Was im eigenen Chat-Fenster fehlt, sind BetterTTV- und FrankerFaceZ-Emotes: die spritzt OBS nur in seine eigenen Fenster ein.

Zwei Dinge bleiben anders als vorher:

- **Stream starten** schickt an uns, nicht an Twitch. Twitch geht live, sobald wir dorthin senden.
- **Enhanced Broadcasting** und Mehrspur-Video bietet OBS nur mit verbundenem Twitch-Konto an. Über uns läuft das nicht. Dafür kommst du mit einem Upload gleichzeitig auf mehrere Plattformen.

### 5. Check vor dem Abend

1. Hardware-HEVC, nicht x264.
2. VBR mit Ziel und Maximum, nicht CBR und nicht ABR.
3. Keyframe 2 s.
4. Eine Testminute: Skipped frames 0, Bitrate unter dem Maximum und kein Stottern im Spiel.
5. Bricht das Bild bei Fights: Maximum um 1000 Kbps senken oder 30 fps wählen. Den Encoder nicht wechseln.

Viele Streamlabs-Versionen können kein SRT. Kannst du dort die SRT-Adresse nicht eintragen, nimm OBS. Auch HEVC ist aus Streamlabs oft nicht nutzbar. Dann ist der Bandbreitenvorteil weg.
