"""Prüft die Kohortenerkennung aus tools/chatter_account_age.py.

Der Kern ist die Frage, wann eine Häufung von Registrierungsmonaten auffällig ist.
Ein Kanal, dessen Publikum ohnehin aus einer Zeit stammt (etwa weil das Spiel
damals erschien), darf nicht allein deswegen gemeldet werden.
"""
from __future__ import annotations

import importlib.util
import sys
from collections import Counter
from pathlib import Path

_spec = importlib.util.spec_from_file_location(
    "chatter_account_age",
    Path(__file__).resolve().parents[1] / "tools" / "chatter_account_age.py",
)
caa = importlib.util.module_from_spec(_spec)
sys.modules["chatter_account_age"] = caa
_spec.loader.exec_module(caa)


def monate(**kwargs: int) -> Counter:
    return Counter({k.replace("_", "-"): v for k, v in kwargs.items()})


class TestKohorten:
    def test_welle_wird_erkannt(self):
        """Gemessener Fall: 74 von 312 Konten aus zwei Monaten, der Rest verteilt."""
        m = monate(**{f"y{2020 + i // 12}_{i % 12 + 1:02d}": 3 for i in range(40)})
        m = Counter({k.replace("y", ""): v for k, v in m.items()})
        m["2024-08"] = 35
        m["2024-09"] = 39
        got = caa.finde_kohorten(m)
        assert [g[0] for g in got] == ["2024-08", "2024-09"]

    def test_gleichmaessige_verteilung_ergibt_keine_welle(self):
        m = Counter({f"2024-{i:02d}": 10 + i % 3 for i in range(1, 13)})
        assert caa.finde_kohorten(m) == []

    def test_kleine_absolute_haeufung_reicht_nicht(self):
        """Bei einem Kanal mit wenigen Chattern ist ein Monat mit 5 Konten schnell
        das Vierfache des Medians — ohne dass das etwas bedeutet."""
        m = Counter({f"2024-{i:02d}": 1 for i in range(1, 13)})
        m["2024-06"] = 5
        assert caa.finde_kohorten(m) == []

    def test_median_statt_mittelwert_als_bezug(self):
        """Eine sehr große Welle zieht den Mittelwert so hoch, dass sie sich selbst
        unauffällig macht. Gegen den Median bleibt sie sichtbar."""
        m = Counter({f"20{y}-{mo:02d}": 2 for y in range(18, 24) for mo in range(1, 13)})
        m["2024-08"] = 400
        got = caa.finde_kohorten(m)
        assert [g[0] for g in got] == ["2024-08"]

    def test_leere_eingabe(self):
        assert caa.finde_kohorten(Counter()) == []
