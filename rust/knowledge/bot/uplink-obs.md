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
| **VBR** | Zielbitrate plus Maximalbitrate. In ruhigen Szenen spart VBR, im Fight gibt es mehr, begrenzt durch das Maximum. Bei AMD gibt es kein Feld für das Maximum, dort liegt es fest beim Anderthalbfachen der Zielbitrate. | Planbar und passend für deine Leitung. | Empfehlung. |
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

##### Wenn du eine AMD-Karte hast

Bei **AMD HW H.264/H.265/AV1** gibt es im VBR-Modus **kein Feld für die Maximalbitrate**. Es steht nur eine Zahl da, und OBS setzt die Spitze selbst: **anderthalbmal die Zielbitrate**. Wer 16000 einträgt, sendet in Spitzen bis 24000, ohne dass es irgendwo steht. Das Feld "AMF/FFmpeg-Optionen" hilft nicht, es kennt `maxrate` und `bufsize` nicht.

Damit gibt es zwei Wege:

- **CBR**: Die eingetragene Zahl ist auch die Obergrenze. Einfachster Weg, und zu uns kostet er nichts an Qualität, weil wir für jede Plattform ohnehin neu rechnen. Nur die Leitung zahlt in ruhigen Szenen drauf.
- **VBR** mit einer Zielbitrate, die mal 1,5 noch in die Leitung passt. Die Rechnung: **Zielbitrate = gemessener Upload × 0,8 ÷ 1,5**.

##### Deine Leitung, deine Zahl

Miss deinen Upload, wenn nichts anderes läuft. Danach sind 80 Prozent davon die Obergrenze. Der Rest ist Reserve für Schwankungen und alles, was nebenbei hochlädt.

| Gemessener Upload | NVIDIA, Intel, Apple | AMD (VBR) | AMD (CBR) |
| --- | --- | --- | --- |
| 6 Mbit | Ziel 4000, max 5000, 30 fps | Ziel 3200 | 4800 |
| 10 Mbit | Ziel 6000, max 8000 | Ziel 5300 | 8000 |
| 14 Mbit | Ziel 9000, max 12000 | Ziel 7400 | 11000 |
| Ab 20 Mbit | Ziel 9000, max 12000 | Ziel 9000 | 12000 |

Die AMD-VBR-Spalte sieht kleiner aus und ist es nicht: 5300 mit Spitze 8000 ist dieselbe Last auf der Leitung wie 6000 mit Maximum 8000. Nur die Zahl im Feld ist eine andere, weil das Feld etwas anderes bedeutet.

Mehr als die oberste Zeile braucht niemand. Zu uns geht HEVC, wir rechnen daraus für jede Plattform H.264, und HEVC packt dasselbe Bild in deutlich weniger Bits.

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
| Ab 14 Mbit, wenn du 2K auch rausschicken willst | 2560×1440 | 60 | HEVC, VBR 9000, maximal 12000 |
| Wenn die GPU Drops hat oder das Spiel ruckelt | Eine Stufe kleiner oder 30 fps |  | Derselbe Encoder, nicht auf Software wechseln |

1440p an uns zu schicken lohnt sich zweifach: entweder rechnen wir daraus 1080p und behalten dabei Schärfe, oder wir senden die vollen 2K weiter, wenn du diese Stufe im Dashboard wählst. Es lohnt sich nicht, wenn der Encoder Frames schluckt. **Skipped frames** bleibt in OBS bei 0.

Die Qualität je Ziel kannst du auch mitten im Stream umstellen. Der Stream bleibt online, wir tauschen nur unseren Encoder aus. Änderst du dabei Auflösung oder Bildrate, sehen deine Zuschauer kurz ein Stocken, weil die Plattform das neue Bildformat annehmen muss. Änderst du nur die Bitrate, merkt niemand etwas. Neue Ziele und neue Stream-Schlüssel gelten weiterhin erst ab dem nächsten Stream: dafür müssten wir die Verbindung zur Plattform neu aufbauen, und das wäre dort ein Stream-Ende.

### 3. Ziel: unser Dienst

Öffne in OBS **Einstellungen**, dann **Stream**.

1. Dienst: **Benutzerdefiniert**
2. Server: die **SRT-Adresse** aus dem Dashboard. Kopiere sie, statt sie abzutippen.
3. Stream-Schlüssel: bleibt leer. Dein Schlüssel steckt bei SRT bereits als `streamid` in der Adresse.

Ins Dashboard kommt nur die SRT-Adresse, und über die läuft dein Stream. SRT verträgt ein Wackelnetz besser. Im Dashboard steht genau eine Adresse, und das ist die SRT-Adresse. Kann dein Programm kein SRT, nimm OBS.

Mit **Stream starten** sendest du danach an uns. Die Plattformen starten wir. Titel und Kategorie stellst du weiter in den Dashboards der Plattformen ein, solange die Synchronisation dafür noch nicht verfügbar ist.

### 4. Chat und die OBS-Fenster zurückholen

Sobald der Dienst auf **Benutzerdefiniert** steht, verschwinden in OBS die Twitch-Fenster: Chat, Aktivitätsfeed und Stream-Informationen. Dein Chat läuft trotzdem normal weiter. Chat und Video sind bei Twitch getrennte Wege, wir fassen nur das Video an. Weg sind nur die Fenster in OBS, weil OBS die ausschließlich bei verbundenem Twitch-Konto einblendet.

Wir geben dir vier eigene Fenster zurück. Anders als die von Twitch zeigen sie alle Plattformen zugleich, die du verbunden hast: Chat mit Antwortfeld, Aktivität mit Follows, Abos und Bits, Stream-Infos zum Ändern von Titel und Kategorie, und die Kanalpunkte.

Du holst sie in einer Minute zurück:

1. Im Dashboard unter **Uplink**, Karte **OBS einrichten**, Schritt 5 **Fenster einrichten**: die vier Adressen stehen dort fertig zum Kopieren.
2. In OBS: **Docks**, dann **Benutzerdefinierte Browser-Docks**. Pro Zeile Namen und Adresse eintragen.
3. Fenster anordnen, dann unter **Docks** das Layout speichern.

| Fenster | Woher kommt die Adresse |
| --- | --- |
| Chat | Dashboard, Karte **OBS einrichten**, Schritt 5 **Fenster einrichten** |
| Aktivität | derselbe Schritt |
| Stream-Infos | derselbe Schritt |
| Kanalpunkte | derselbe Schritt |

Die Adressen sind im Dashboard verdeckt, weil in jeder von ihnen dein Zugang steckt: wer sie mitliest, kann in deinem Namen im Chat schreiben. Zeig sie also nicht im Stream. Kopieren geht auch verdeckt. Ist eine Adresse irgendwo gelandet, wo sie nicht hingehört, drückst du im Dashboard **Neu erzeugen**; dann gelten die alten vier nicht mehr und du trägst die neuen in OBS ein.

#### „Muss ich mich da anmelden?“

Nein. Genau das ist der Unterschied zu den eingebauten Twitch-Fenstern: die laden Twitch-Seiten in den Browser von OBS und brauchen dort ein eigenes Twitch-Cookie. Unsere vier Fenster kommen von uns und bringen ihren Zugang in der Adresse mit. Einmal eintragen, fertig.

Zwei Anmeldungen gehen an dieser Stelle gern durcheinander:

- **Konto verbinden** in den Stream-Einstellungen von OBS. Damit holt sich OBS deinen Twitch-Streamschlüssel. Die brauchst du bei uns nicht, deshalb ist sie weg.
- **Mit Twitch verbinden** im Dashboard, in der Plattform-Karte. Darüber holen wir Chat, Aktivität, Stream-Infos, Kanalpunkte und deinen Stream-Schlüssel in einem Schritt. Ohne diesen Klick bleiben die vier Fenster leer.

Deine Zugänge bleiben dabei auf unserem Server. In den Fenster-Adressen steckt nur dein Zugang zu uns, nichts von Twitch.

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
