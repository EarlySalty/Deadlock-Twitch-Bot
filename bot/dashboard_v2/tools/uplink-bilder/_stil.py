# Gemeinsame Bausteine fuer die OBS-Nachzeichnungen.
#
# Nachgezeichnet statt fotografiert: ein Screenshot veraltet mit jeder
# OBS-Version und traegt fremde Bildrechte. Diese Bilder zeigen genau die
# Felder, um die es geht, und sind in der Farbwelt der Hilfeseite.
OBS_BG = "#2b2b2b"
OBS_PANEL = "#3c3c3c"
OBS_FELD = "#232323"
OBS_TEXT = "#e6e6e6"
OBS_GRAU = "#9a9a9a"
OBS_RAND = "#1e1e1e"
GOLD = "#c8a86b"
GOLD_HELL = "#efd49d"
BLAU = "#4a7fb5"

def kopf(b, h, titel):
    return f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {b} {h}" width="{b}" height="{h}" role="img" aria-labelledby="t">
<title id="t">{titel}</title>
<style>
  text {{ font-family: system-ui, sans-serif; }}
  .lbl {{ fill: {OBS_TEXT}; font-size: 15px; }}
  .dim {{ fill: {OBS_GRAU}; font-size: 14px; }}
  .feld {{ fill: {OBS_TEXT}; font-size: 14px; font-family: monospace; }}
  .mark {{ fill: {GOLD_HELL}; font-size: 14px; font-weight: 700; }}
  .titel {{ fill: {OBS_TEXT}; font-size: 16px; font-weight: 600; }}
</style>
<rect width="{b}" height="{h}" rx="10" fill="{OBS_BG}" stroke="{OBS_RAND}"/>'''

def feld(x, y, b, h=34, fuellung=OBS_FELD):
    return f'<rect x="{x}" y="{y}" width="{b}" height="{h}" rx="4" fill="{fuellung}" stroke="#1a1a1a"/>'

def ring(x, y, b, h):
    """Goldener Markierungsrahmen um das Feld, auf das es ankommt."""
    return (f'<rect x="{x-5}" y="{y-5}" width="{b+10}" height="{h+10}" rx="8" fill="none" '
            f'stroke="{GOLD}" stroke-width="2.5"/>')

def nummer(x, y, n):
    return (f'<circle cx="{x}" cy="{y}" r="14" fill="{GOLD}"/>'
            f'<text x="{x}" y="{y+5}" text-anchor="middle" font-size="15" font-weight="800" '
            f'fill="#241c11" font-family="system-ui, sans-serif">{n}</text>')
