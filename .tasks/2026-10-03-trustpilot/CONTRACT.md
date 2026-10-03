# Trustpilot auf der Streamer-Seite und im Twitch-Chat

Auftrag vom 3. Oktober 2026: Die gelieferten Trustpilot-Einbindungen auf `/streamer` ergänzen und im Twitch-Bot eine Bewertungsmöglichkeit schaffen.

Die Seite zeigt den Review Collector mit den gelieferten öffentlichen Browser-Einbindungswerten. Bootstrap und Einladungsskript werden einmal pro Browserdokument geladen. Das Einladungsskript registriert den gelieferten Schlüssel. Es gibt keinen Aufruf von `createInvitation` und keine Übergabe von E-Mail-Adressen oder Kundendaten. Ein dauerhaft sichtbarer Bewertungslink funktioniert auch bei blockierten Skripten. Er führt zu `https://de.trustpilot.com/evaluate/deutsche-deadlock-community.de`.

Der gelieferte Kurzlink `https://trstp.lt/tM_2rZrhWk` öffnet laut Browserprüfung der Hauptsession die Trustpilot-Einbauanleitung. Deshalb erscheint er weder im Bewertungsbutton noch im Chat. Der TrustBox-Anker führt wie geliefert zum öffentlichen Profil unter `/review/deutsche-deadlock-community.de`.

`!trustpilot` und `!bewerten` geben den direkten Bewertungslink mit einer von vier neutralen deutschen Formulierungen zurück. Der bestehende Katalog macht den Befehl auf `/streamer/commands` und in den Dashboard-Befehlsnamen sichtbar. Der Chat verlangt weder eine bestimmte Sternezahl noch eine positive Bewertung. Bekannte Automationsbots dürfen den Bewertungslink nicht auslösen. Es entsteht kein neuer Werbetimer und keine automatische Ankündigung.

Die Hauptsession prüft unabhängig Nutzerziel, Rust-Code, Fehler und Sicherheit. Der ergänzende Caddy-Auftrag öffnet nur die benötigten Script- und Frame-Quellen. Nach gemeinsamer Abnahme folgen Merge, Push, sauberer Release, Neustart beider System-Units, Live-Prüfung und Entfernen des eigenen Branches und Worktrees. Fremde Änderungen im Hauptcheckout bleiben erhalten.
