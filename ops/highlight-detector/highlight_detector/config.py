import os
import tomllib

BASIS = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CONFIG_DIR = os.path.join(BASIS, "config")


def _lade(pfad):
    with open(pfad, "rb") as f:
        return tomllib.load(f)


def lade_regionen(pfad=None):
    pfad = pfad or os.path.join(CONFIG_DIR, "regionen.toml")
    roh = _lade(pfad)
    return {
        "spieler_namen": [n.lower() for n in roh["allgemein"]["spieler_namen"]],
        "frame_breite": roh["allgemein"]["frame_breite"],
        "frame_hoehe": roh["allgemein"]["frame_hoehe"],
        "ocr": roh["ocr"],
        "regionen": roh["region"],
        "parser": roh["parser"],
    }


def lade_gewichte(pfad=None):
    pfad = pfad or os.path.join(CONFIG_DIR, "gewichte.toml")
    roh = _lade(pfad)
    return {
        "gewichte": roh["gewichte"],
        "signale_aktiv": roh["signale_aktiv"],
        "fenster": roh["fenster"],
        "schnitt": roh["schnitt"],
        "schluesselwoerter": [w.lower() for w in roh["schluesselwoerter"]["begriffe"]],
    }
