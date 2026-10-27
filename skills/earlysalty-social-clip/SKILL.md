---
name: earlysalty-social-clip
description: Rendert EarlySalty-Twitch/VOD-Momente als native 9:16 Social-Clips im festgelegten DDC-Template. Nutzen bei "mach einen Social Clip", "render den Clip", "EarlySalty Template", "Twitch Clip zu TikTok/Reel/Short" und bei Folgeiterationen auf V10/V11.
---

# EarlySalty Social Clip Template

Dieses Skill ist die feste Produktionsvorlage für `EarlySalty`. Nicht bei jeder Iteration das Layout neu erfinden. Ausgangspunkt ist das akzeptierte V10-Layout, mit den Qualitätskorrekturen aus V11.

## Ziel

Aus Twitch-Clips oder dem Parent-VOD einen nativen `1080x1920`-Clip erzeugen, bei dem EarlySaltys Cam oben sichtbar ist, Gameplay darunter groß bleibt, Untertitel dezent sind und DDC nur als kleine Social-Leiste plus sauberes Outro erscheint.

## Feste Struktur

1. Cold open / Hook: `3.2–4.2 s`.
2. Kein sichtbares `VORSCHAU`-Label, keine Preview-Fortschrittsleiste.
3. Hook soll die stärkste Reaktion enthalten. Bei diesem Referenzclip ist `Oh my god, a double again.` korrekt.
4. Nach der gesprochenen Hook/Reaktion noch ungefähr `0.8–1.2 s` Bild stehen lassen, nicht direkt nach dem letzten Wort hart schneiden.
5. Übergang: ca. `0.30–0.35 s` Fade-to-black am Hook-Ende und ca. `0.30 s` Fade-in in den Hauptclip.
6. Danach den vollständigen Moment zeigen. Wenn der Twitch-Clip mitten in der Aktion endet, Parent-VOD holen und nur so viel Tail ergänzen, dass der Moment sichtbar abgeschlossen ist. Kein unnötiger Stream-Talk.
7. Outro: `3.2 s`.

## Canvas und Layout

- Ausgabe: `1080x1920`, `30 fps`, H.264 + AAC 48 kHz.
- Quelle ist normalerweise `1920x1080`.
- Cam oben: ca. ein Viertel der Höhe.
- Gameplay darunter: ca. zwei Drittel bis drei Viertel der nutzbaren Fläche.
- Keine anisotrope Skalierung / kein Stretching.

### EarlySalty Referenz-Crops bei 1920x1080

- Cam source crop: `(40,260)-(540,560)` = `500x300`.
- Cam destination box: `(140,20)-(940,500)` = `800x480`.
- Gameplay source crop: `(480,0)-(1440,1080)` = `960x1080`.
- Gameplay destination box: `(30,520)-(1050,1668)` = `1020x1148`.
- Social bar: ungefähr `y=1690..1870`.

Wenn die Stream-Szene anders ist, Cam nicht blind anhand dieser Koordinaten schneiden. Gesicht/Facecam erkennen und den Crop so verschieben, dass Kopf und Schultern vollständig sichtbar sind. Das Gameplay-Layout ansonsten beibehalten.

## Cam-Korrektur

EarlySaltys Webcam kann im Original stark überbelichtet sein. Nur die Cam korrigieren, niemals das Gameplay abdunkeln.

Referenzkurve für Highlights, auf RGB im Bereich 0..1:

`x = x * (1 - 0.24 * x)`

Damit werden helle Haut-/Lichtbereiche deutlich gedrückt, während dunkle Bereiche weniger stark verändert werden. Bereits geclippte Details können nicht rekonstruiert werden; Ziel ist ein weniger ausgewaschener Eindruck.

## Untertitel

- Dezent, nicht TikTok-Slop.
- Etwa `38–40 px` bei 1080x1920.
- Maximal zwei Zeilen.
- Weiß, kleiner dunkler halbtransparenter Rounded-Rect-Hintergrund.
- Am unteren Bereich des Gameplays, nicht als eigener großer Untertitel-Balken.
- Transkript vor Veröffentlichung grob auf offensichtliche Whisper-Fehler korrigieren.

## Social-Leiste

Kompakt am unteren Rand:

- echtes Twitch-Logo + `EarlySalty` + `twitch.tv/earlysalty`
- echtes Discord-Logo + `Community` + `discord.gg/z5TfVHuQq2`
- keine ausgeschriebenen Plattformnamen als Ersatz für Logos.

## DDC-Outro Template

Das alte Outro mit riesiger goldener Scheibe ist verboten.

Verwenden:

- Hintergrund: sehr dunkles Ink / Schwarz.
- `logo/logo-192.png` als zentriertes Logo-Badge mit dezenter goldener Kontur/Glow.
- Kein großer gefüllter goldener Kreis hinter dem Logo.
- Kleines `POWERED BY` in Gold.
- Darunter das echte `logo/wordmark.svg`.
- Keine zweite große ausgeschriebene `Deutsche Deadlock Community`-Zeile, wenn das Wordmark bereits sichtbar ist.
- Eine kompakte CTA-Karte mit echtem Discord-Logo, echtem Twitch-Logo, Invite und EarlySalty-Link.
- Optional klein: `deutsche-deadlock-community.de` und `DEADLOCK • DEUTSCH • COMMUNITY`.

Brandfarben aus `dl-brand`: Bone `#f2eee6`, Gold `#c8a86b`, Ink ungefähr `#0b0b0b`.

## Twitch/VOD-Regel

Ein Twitch-Clip ist nicht zwingend lang genug. Vor dem Rendern Clip-Metadaten prüfen:

- `durationSeconds`
- `videoOffsetSeconds`
- Parent `video.id`

Wenn ein Parent-VOD vorhanden ist und die Aktion am Clip-Ende abgeschnitten wirkt, VOD-Quelle laden. Bei HLS/Keyframe-Versatz die Clip- und VOD-Audiospur per Wellenform/Feature-Korrelation ausrichten. Nicht anhand eines geratenen Offsets schneiden.

Für den Referenzclip `QuaintHandsomeSagePRChase-XOXnF9iDBZUIyYj0`:

- Twitch-Clip: 23 s.
- Parent-VOD: `2871120418`.
- VOD ist länger; V11 ergänzt bewusst nur wenige Sekunden Tail.

## Qualitäts-Gates vor Ausgabe

Alle Punkte müssen erfüllt sein:

- Hook mindestens etwa 3 s und mit echter Retention-Wirkung.
- Keine `VORSCHAU`-UI.
- Fade zwischen Hook und Hauptclip sichtbar, aber kurz.
- Cam sichtbar und korrekt ausgeschnitten.
- Gesicht nicht unnötig ausgebrannt.
- Gameplay nicht gestretcht.
- Gameplay-Zuschnitt gegenüber V10 nicht ohne Grund ändern.
- Untertitel klein und lesbar.
- echtes Discord-Logo.
- echtes Twitch-Logo.
- Outro ohne riesigen Goldkreis, Überlappungen oder doppelte Community-Titel.
- Wenn Parent-VOD vorhanden: Moment nicht sichtbar mitten in der Aktion abbrechen.
- Final: 1080x1920, 30 fps, H.264/AAC, öffentlicher HTTP-Abruf `200 video/mp4`.

## Aktuelle Referenzimplementierung

Auf dem aktuellen Server:

- Renderer: `/home/nathanael/repos/Deadlock-Twitch-Bot/tools/_render_social_v11.py`
- V11 Preview: `/home/nathanael/repos/Website/dl-brand/social-preview/earlysalty-social-preview-v11.mp4`
- Outro PNG: `/home/nathanael/repos/Website/dl-brand/social-preview/earlysalty-outro-template-v11.png`
- Brand Assets: `/home/nathanael/repos/Website/dl-brand`

Beim Iterieren immer von diesem Template ausgehen und nur explizit kritisierte Punkte ändern.
