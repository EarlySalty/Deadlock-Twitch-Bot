# Erzeugt die Anleitungsbilder unter public/uplink/bilder/.
#
# Die Generatoren liegen bewusst ausserhalb von public/: Vite kopiert public/
# unveraendert ins Bundle, dort haetten .py-Dateien und __pycache__ nichts zu
# suchen. Aufruf aus diesem Verzeichnis: python3 bauen.py
import os
os.chdir(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "public", "uplink", "bilder"))

from _stil import *

# ---------------------------------------------------------------- 1. Stream
B, H = 900, 330
s = [kopf(B, H, "OBS-Einstellungen, Reiter Stream: Dienst Benutzerdefiniert, Serveradresse eingetragen, Streamschluessel leer")]
s.append(f'<rect x="0" y="0" width="{B}" height="46" rx="10" fill="{OBS_PANEL}"/>')
s.append(f'<rect x="0" y="36" width="{B}" height="10" fill="{OBS_PANEL}"/>')
s.append(f'<text x="24" y="30" class="titel">Einstellungen</text>')
s.append(f'<text x="180" y="30" class="dim">Stream</text>')
s.append(f'<rect x="168" y="42" width="66" height="3" fill="{BLAU}"/>')

zeilen = [
    ("Dienst", "Benutzerdefiniert...", 90, True, 1),
    ("Server", "srt://deutsche-deadlock-community.de:8899?mode=caller&latency=2000&streamid=rsr_…", 160, True, 2),
    ("Streamschlüssel", "", 230, True, 3),
]
for label, wert, y, markiert, n in zeilen:
    s.append(f'<text x="24" y="{y+22}" class="lbl">{label}</text>')
    s.append(feld(210, y, 620))
    if wert:
        # & muss im SVG escaped werden, sonst ist die Datei nicht wohlgeformt
        # und der Browser zeigt statt des Bildes einen Parserfehler.
        gezeigt = wert[:70].replace("&", "&amp;").replace("<", "&lt;")
        if len(wert) > 70:
            gezeigt += "\u2026"
        s.append(f'<text x="222" y="{y+22}" class="feld">{gezeigt}</text>')
    if markiert:
        s.append(ring(210, y, 620, 34))
        s.append(nummer(858, y + 17, n))

s.append(f'<text x="222" y="252" class="dim" font-style="italic">leer lassen</text>')
s.append(f'<text x="24" y="300" class="mark">1 Benutzerdefiniert wählen &#183; 2 Adresse aus dem Dashboard einfügen &#183; 3 nichts eintragen</text>')
s.append('</svg>')
open("1-stream.svg", "w").write("\n".join(s))

# ------------------------------------------------------------ 2. Docks-Menue
B, H = 900, 330
s = [kopf(B, H, "OBS-Menue Docks mit dem Eintrag Benutzerdefinierte Browser-Docks")]
s.append(f'<rect x="0" y="0" width="{B}" height="40" rx="10" fill="{OBS_PANEL}"/>')
s.append(f'<rect x="0" y="30" width="{B}" height="10" fill="{OBS_PANEL}"/>')
for i, m in enumerate(["Datei", "Bearbeiten", "Ansicht", "Docks", "Profil", "Werkzeuge", "Hilfe"]):
    x = 24 + i * 105
    klasse = "lbl" if m == "Docks" else "dim"
    s.append(f'<text x="{x}" y="26" class="{klasse}">{m}</text>')
    if m == "Docks":
        s.append(f'<rect x="{x-12}" y="4" width="76" height="32" rx="4" fill="{BLAU}" opacity="0.35"/>')
        s.append(f'<text x="{x}" y="26" class="lbl">{m}</text>')
        menue_x = x - 12

s.append(f'<rect x="{menue_x}" y="40" width="330" height="176" rx="6" fill="{OBS_PANEL}" stroke="{OBS_RAND}"/>')
eintraege = ["Szenen", "Quellen", "Mixer", "Übergänge", "Steuerung"]
for i, e in enumerate(eintraege):
    s.append(f'<text x="{menue_x+18}" y="{68+i*26}" class="dim">{e}</text>')
    s.append(f'<rect x="{menue_x+300}" y="{60+i*26}" width="12" height="12" rx="2" fill="none" stroke="{OBS_GRAU}"/>')
s.append(f'<line x1="{menue_x+10}" y1="{62+len(eintraege)*26}" x2="{menue_x+320}" y2="{62+len(eintraege)*26}" stroke="{OBS_RAND}"/>')
yy = 62 + len(eintraege) * 26 + 24
s.append(f'<rect x="{menue_x+6}" y="{yy-19}" width="318" height="30" rx="4" fill="{BLAU}" opacity="0.4"/>')
s.append(f'<text x="{menue_x+18}" y="{yy}" class="lbl">Benutzerdefinierte Browser-Docks…</text>')
s.append(ring(menue_x + 6, yy - 19, 318, 30))
s.append(nummer(menue_x + 350, yy - 4, 1))
s.append(f'<text x="24" y="290" class="mark">In der Menüleiste auf „Docks“, dann ganz unten „Benutzerdefinierte Browser-Docks“.</text>')
s.append('</svg>')
open("2-docks-menue.svg", "w").write("\n".join(s))

# ----------------------------------------------------------- 3. Docks-Dialog
B, H = 900, 400
s = [kopf(B, H, "Dialog Benutzerdefinierte Browser-Docks mit vier eingetragenen Fenstern")]
s.append(f'<rect x="0" y="0" width="{B}" height="46" rx="10" fill="{OBS_PANEL}"/>')
s.append(f'<rect x="0" y="36" width="{B}" height="10" fill="{OBS_PANEL}"/>')
s.append(f'<text x="24" y="30" class="titel">Benutzerdefinierte Browser-Docks</text>')
s.append(f'<text x="30" y="80" class="dim">Dock-Name</text>')
s.append(f'<text x="250" y="80" class="dim">URL</text>')

# Unsere vier Fenster, nicht die von Twitch: die Adressen stehen im Dashboard
# in der Karte "Chat und OBS-Fenster". Der Zugang hinter "t=" ist hier nur
# angedeutet, das Bild zeigt die Form, nicht den Wert.
reihen = [
    ("Chat", "https://deutsche-deadlock-community.de/dock/chat?t=…"),
    ("Aktivität", "https://deutsche-deadlock-community.de/dock/activity?t=…"),
    ("Stream-Infos", "https://deutsche-deadlock-community.de/dock/stream-info?t=…"),
    ("Kanalpunkte", "https://deutsche-deadlock-community.de/dock/points?t=…"),
]
for i, (name, url) in enumerate(reihen):
    y = 94 + i * 46
    s.append(feld(30, y, 200))
    s.append(f'<text x="42" y="{y+22}" class="feld">{name}</text>')
    s.append(feld(244, y, 590))
    s.append(f'<text x="256" y="{y+22}" class="feld" font-size="12">{url}</text>')
    s.append(f'<text x="848" y="{y+23}" class="dim" font-size="18">&#10005;</text>')

s.append(ring(30, 94, 804, 4 * 46 - 12))
s.append(nummer(866, 100, 2))
s.append(f'<rect x="30" y="292" width="40" height="32" rx="4" fill="{OBS_PANEL}" stroke="{OBS_RAND}"/>')
s.append(f'<text x="50" y="314" text-anchor="middle" class="lbl" font-size="20">+</text>')
s.append(ring(30, 292, 40, 32))
s.append(nummer(96, 308, 1))
s.append(f'<rect x="726" y="292" width="108" height="34" rx="4" fill="{BLAU}"/>')
s.append(f'<text x="780" y="314" text-anchor="middle" class="lbl">Übernehmen</text>')
s.append(nummer(862, 309, 3))
s.append(f'<text x="30" y="368" class="mark">1 Pro Fenster einmal auf „+“ &#183; 2 Name und Adresse einfügen &#183; 3 Übernehmen</text>')
s.append('</svg>')
open("3-docks-dialog.svg", "w").write("\n".join(s))

# ---------------------------------------------------------------- 4. Ausgabe
B, H = 900, 330
s = [kopf(B, H, "OBS-Einstellungen, Reiter Ausgabe: Hardware-HEVC, VBR, Keyframe 2 Sekunden")]
s.append(f'<rect x="0" y="0" width="{B}" height="46" rx="10" fill="{OBS_PANEL}"/>')
s.append(f'<rect x="0" y="36" width="{B}" height="10" fill="{OBS_PANEL}"/>')
s.append(f'<text x="24" y="30" class="titel">Einstellungen</text>')
s.append(f'<text x="180" y="30" class="dim">Ausgabe</text>')
s.append(f'<rect x="168" y="42" width="80" height="3" fill="{BLAU}"/>')

felder = [
    ("Ausgabemodus", "Erweitert", 76),
    ("Videokodierer", "NVIDIA NVENC HEVC", 122),
    ("Ratensteuerung", "VBR", 168),
    ("Bitrate / Maximum", "6000 Kbps / 8000 Kbps", 214),
    ("Keyframe-Intervall", "2 s", 260),
]
for label, wert, y in felder:
    s.append(f'<text x="24" y="{y+22}" class="lbl">{label}</text>')
    s.append(feld(260, y, 520))
    s.append(f'<text x="272" y="{y+22}" class="feld">{wert}</text>')
    s.append(f'<text x="758" y="{y+23}" class="dim">&#9662;</text>')
s.append(ring(260, 122, 520, 34))
s.append(nummer(818, 139, 1))
s.append(ring(260, 168, 520, 34))
s.append(nummer(818, 185, 2))
s.append(f'<text x="24" y="316" class="mark">1 Hardware-HEVC, nicht x264 &#183; 2 VBR, nicht CBR und nicht ABR</text>')
s.append('</svg>')
open("4-ausgabe.svg", "w").write("\n".join(s))
print("gebaut")
