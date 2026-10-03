# Brain-Konfiguration mit tb-config-check

`tb-config-check --config /absoluter/pfad/bot.toml` bleibt die lesende Gesamtprüfung. `--brain-inspect` ergänzt eine JSON-Ausgabe mit `revision`, `bot_brain_client.{mode,endpoint}`, `bot_brain_chat_enabled` und `dashboard_brain_client.{mode,endpoint}`. `revision` ist der SHA-256 des gespeicherten UTF-8-TOML einschließlich Kommentaren und Formatierung, nicht der semantische `fingerprint` der bisherigen Prüfung. Die Ausgabe beschreibt die Datei, nicht die laufenden Dienste.

## Änderung

Der Aufruf lautet `tb-config-check --config /var/lib/deadlock-twitch/config/bot.toml --brain-apply --expected-revision <alter-sha256>`. Andere Schreibziele werden abgewiesen. Die Eingabe kommt als JSON über Standardeingabe und ist auf 16 KiB begrenzt.

```json
{
  "bot_brain_client": {
    "mode": "typed",
    "endpoint": "http://127.0.0.1:8788"
  },
  "bot_brain_chat_enabled": true,
  "dashboard_brain_client": {
    "mode": "typed",
    "endpoint": "http://127.0.0.1:8788"
  }
}
```

Die fünf Felder sind optional; weggelassene Felder bleiben erhalten. `mode` akzeptiert `legacy`, `shadow` und `typed`, `endpoint` eine Zeichenkette und `bot_brain_chat_enabled` einen booleschen Wert. Unbekannte Felder, falsche Typen, `null`, doppelte JSON-Schlüssel und leere Änderungen werden abgewiesen. Endpunkte durchlaufen die vorhandene lokale URL-Prüfung, auch im Modus `legacy`; enthaltene Zugangsdaten, Query und Fragment sind nicht zulässig. Für `shadow` und `typed` müssen die vorhandenen Scopes bereits gültig sein. Scopes, Zeitlimits und weitere Einstellungen werden nicht ergänzt oder verändert.

Nach erfolgreicher Gesamtprüfung und atomarem Speichern folgt dasselbe begrenzte JSON-Format wie bei der Inspektion, mit der neuen Revision. Erfolg liefert Exit 0, Fehler Exit 2 ohne Eingabeinhalte. Ein Ausgabefehler nach dem Speichern macht die Änderung nicht rückgängig; vor einer Wiederholung den gespeicherten Stand erneut inspizieren.

## Installation und Betrieb

Installation und produktive Ausführung erfolgen erst nach gemeinsamer Abnahme über den bestehenden privilegierten Verwaltungsweg. Das Binary muss aus demselben geprüften Integrationsstand stammen; die vorhandenen Skripte `ops/systemd/deploy-twitch-release` und `ops/systemd/install-twitch-release.sh` verteilen `tb-config-check` derzeit nicht als Releaseartefakt. Diese Installation bleibt Aufgabe der Integration, ohne Erweiterung dieser Skripte in diesem Bauschritt.

Das ausführende Konto benötigt Leserechte auf die Konfiguration, Schreib- und Suchrechte im Konfigurationsverzeichnis, Zugriff auf `.bot.toml.lock` und die erforderlichen Rechte zum Erhalten von Eigentümer, Gruppe und Dateimodus. Das Werkzeug beschafft keine erhöhten Rechte. Eine vorhandene Sperrdatei behält ihre Metadaten; eine neue Sperre erhält Eigentümer und Gruppe der Konfiguration sowie Modus `0600`, bei gruppenlesbarer Konfiguration `0660`. Symlinks, harte Links beim Schreiben, Sonderdateien, Git-Checkouts als Schreibablage, unzugängliche Sperren und Revisionskonflikte führen zum Abbruch.

Die bestehende Editorsperre umfasst Hashprüfung, Gesamtvalidierung und atomaren Austausch. Eigentümer, Gruppe und Rechte der Konfiguration bleiben erhalten. Das Werkzeug startet keine Dienste neu und aktiviert keine laufende Konfiguration; Neustart, Live-Prüfung und gegebenenfalls Rückkehr zu `legacy` koordiniert die gemeinsame Integration.
