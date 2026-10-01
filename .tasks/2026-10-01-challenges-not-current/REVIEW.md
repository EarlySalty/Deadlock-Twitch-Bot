# Review

Eigenprüfung: API-Auth und no-store unverändert; Anzeige verlangt sechs aktuelle Quellen, das Belohnungs-Gate weiter sieben. category_collection bleibt bei historischer Lücke ungesund. Andere Quell- oder DB-Fehler werden nicht als Kategorie-Teilzustand verschluckt. Stream-Aufgaben werden pro Vierwochenbasis und Wertungswoche geprüft; fehlende Abdeckung verhindert Belohnung und Dreierbonus. Bereits bestätigte Punkte bleiben unverändert. Historische Streak-Lücken verbrauchen nach erneuter Berechnung keinen Freeze und setzen keine bestätigte Serie zurück.

Watchdog: vorhandener Timer und vorhandener Broker; konstante Service- und Brokerziele, keine fremden URLs. Postgres-Vorfälle und stabile Idempotenzschlüssel verhindern Mehrfachmeldungen über Neustarts hinweg. Fehlgeschlagene Zustellung bleibt unbestätigt und wird wiederholt. Auch innerhalb der letzten zwei Tage übersehene Collector-Lücken werden aus Collector-Läufen nachgetragen. Keine neuen Token-Dateien und keine Twitch-Sendefunktion.

Unabhängiges Review wird ausschließlich über das beauftragte gate_hook.py --review ausgeführt. Keine Unter-Agenten.

Zusätzlicher Anzeige-Leser gefunden: /leaderboard/effort benutzt ebenfalls ensure_ready. Auch dort gilt jetzt ensure_display_ready; der JSON-Vertrag erhält category_data_complete. Bestätigte Stream-Aufgaben behalten ihren Abschluss bei einer späteren Lücke, und bereits belegte Abschlüsse dürfen mit unabhängigen Aufgaben weiterhin einen gültigen Gesamtbonus auslösen. Unbestätigte Stream-Aufgaben bleiben gesperrt.
