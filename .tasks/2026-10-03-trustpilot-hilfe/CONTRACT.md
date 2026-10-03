# Trustpilot bei Hilfe und Dashboard

Der Bewertungsaufruf auf `/streamer` sitzt kompakt am rechten Bildschirmrand, auf Desktop mit seiner Mitte bei ungefähr 55 Prozent der Viewporthöhe. Bei sehr geringer Höhe wird er so geklemmt, dass er oberhalb des Hilfe-Knopfs bleibt. Unterhalb des `sm`-Breakpoints sitzt er direkt über dem Hilfe-Knopf, damit der Hero-Text auf Mobilgeräten lesbar bleibt. Die bisherige große Karte am Seitenende entfällt.

Bei geöffneter Bot-Hilfe erscheint die Trustpilot-Einbindung unter dem Header, außerhalb von Chatverlauf und Formular. Der separate Aufruf wird dann ausgeblendet. Profil- und Bewertungslink bleiben auch bei blockierten Skripten erreichbar. Der gelieferte offizielle Review Collector, seine Einbindungswerte und die SDK-Registrierung bleiben erhalten. Auf niedrigen Bildschirmen wird der Collector zugunsten von Chat und Eingabe ausgeblendet; die beiden Links bleiben sichtbar.

Die Dashboard-Startseite bekommt eine passende Bewertungskarte in der rechten Spalte. Die Neuigkeiten zeigen drei kurze Einträge in einem begrenzten Bereich. Über „Alle Neuigkeiten lesen“ bleiben alle gelieferten Einträge samt vollständigem Text zugänglich.

## Noch offener Score-Teil

Der vorhandene Review Collector zeigt keinen TrustScore. Die öffentliche Widget-Datenquelle bestätigt derzeit keine vorhandenen Bewertungen. Die Markenregeln vom September 2026 erlauben eigene Scorewidgets nur mit passender Planberechtigung. Ein zunächst geprüfter eigener JSON-Scorepfad wurde deshalb vollständig entfernt. Es gibt keine erfundenen Scores, Sterne, Template-IDs oder feste Bewertungszahlen.

Für die gewünschte echte Scoreanzeige muss ein für dieses Konto freigegebener offizieller Score-Embed vorliegen. Die Hauptsession hat den Nutzer bereits danach gefragt. Dieser Teil gilt bis dahin als offen.

Quelle der Markenregel: https://corporate.trustpilot.com/legal/for-businesses/legal-brand-guidelines/sept-2026

## Auslieferung

Beide Frontends werden gemeinsam geprüft und ausgeliefert. Der auf Basis `8db73ec3` geprüfte Installer verlangt einen vollständigen Release mit passenden eingebetteten SHAs aller sieben Rust-Binaries einschließlich `tb-config-check` und allen drei Frontends. Keine alten Binaries umetikettieren und keinen ungeprüften reinen UI-Deploy einführen. Erst nach unabhängiger Abnahme und Selbstreview wird der Release eingefroren, gemergt, gepusht und unter den gemeinsamen Hostlocks gebaut. Bot und Dashboard werden danach neu gestartet und live geprüft. Hauptcheckouts und fremde Arbeiten bleiben unberührt.
