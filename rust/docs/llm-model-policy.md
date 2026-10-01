# DeepSeek Flash aus YAML

Die Fireworks-Modellauswahl des Twitch-Bots kommt aus `rust/knowledge/llm.yaml`.
Die Datei wird mit `rust/knowledge` im Release ausgeliefert und durch
`tb-bot --config <absolute-bot.toml> --check-config` mitgeprüft.
Die übrige Betriebskonfiguration bleibt in der bestehenden TOML-Datei.
Zugangsdaten stehen nicht in YAML, sondern bleiben im bisherigen Dienst-Loader.

## Auswahl und Aktualisierung

`selection: latest` erlaubt die neueste stabile, serverlos verfügbare
DeepSeek-Flash-Fassung. `bootstrap_model` ist kein Pin: Es legt die
Mindestversion und einen Startkandidaten fest, der bei einem Katalogausfall
immer noch eine erfolgreiche synthetische Probe bestehen muss.
Der Stand dieses Releases verwendet als Mindestversion
`accounts/fireworks/models/deepseek-v4p1-flash`.

Der überwachte Bot-Job prüft beim Start und nach `refresh_seconds` erneut.
Nach Fehlern gilt `retry_seconds`. Bei HTTP 404/410 im gemeinsamen KI-Eingang
wird einmal neu aufgelöst und höchstens einmal mit einem anderen geprüften
Modell wiederholt. Katalog, Probe und Wiederholung zählen zur Gesamtfrist
des jeweiligen Aufrufers. Parallele Anfragen teilen sich die Auflösung.
Eine Änderung der YAML-Policy selbst erfordert einen Dienstneustart.

Versionen werden numerisch verglichen, etwa V4.10 vor V4.9. Pro, Lite,
Preview, Thinking, Vision und fremde Anbieter gehören nicht zur Freigabe.
Der paginierte offizielle Katalog wird auf `READY`, `supportsServerless`
und ein gegebenenfalls abgelaufenes `deprecationDate` geprüft. Ein Eintrag
allein genügt nicht: Vor jeder Übernahme muss das Modell eine feste
synthetische JSON-Probe beantworten. Es werden höchstens drei Kandidaten
pro Auflösung geprüft. Weder Community-Texte noch Streamer-Daten werden
für den Katalog oder die Modellprobe verwendet.

## Ausfälle und Nachweis

Nur erfolgreiche Proben aktualisieren den bestehenden `llm_model_cache`.
Der Cache-Schlüssel enthält einen Fingerabdruck der gesamten YAML-Policy.
Ein Neustart behandelt den Cache nur als Kandidaten und prüft ihn erneut.
Abgelaufene, fremde oder durch 404/410 abgelehnte Modelle werden nicht blind
weiterverwendet. Ein vorübergehender Fehler stuft einen noch gültigen,
bereits geprüften Stand nicht auf eine ältere Version zurück.

Ohne verfügbares geprüftes Modell wird ein technischer Fehler zurückgegeben,
kein Spam-Urteil erfunden. Der Modelljob schreibt diesen Zustand ausdrücklich
als Fehler ins Dienstjournal. Der bisherige Umgang des Konversationswächters
mit technischen Fehlern und sein Discord-Alarmverhalten werden durch diesen
Modell-Fix nicht neu gestaltet.

Die gemeinsame Transportstrecke gilt auch für Spam-Judge und
Konversationswächter. Die bestehende GLM-Ausnahme für `title_ai` bleibt bestehen.
Lokale Mock-/Proxy-Endpunkte werden nicht durch echte Anbieterzugriffe ersetzt.

## Tests

`cargo test -p tb-llm` prüft unter anderem strenge YAML-Validierung,
Zahlenversionen, fehlende Katalogzeitstempel, Seitennavigation, Variantenfilter,
Fehlerpausen, Single-Flight, kaputte Proben und einen vollständigen
404-Wechsel im gemeinsamen KI-Pfad. Mit `TB_TEST_DATABASE_URL` kommt der
Postgres-Nachweis für Neustart, Policy-Bindung und Ablauf des Cache hinzu.
Dafür ausschließlich eine Wegwerf-Datenbank verwenden.

## Anbieterreferenz

Stand der Verifikation: 30.09.2026.

- Modell: https://fireworks.ai/models/deepseek-ai/deepseek-v4p1-flash
- Katalogvertrag: https://docs.fireworks.ai/api-reference/list-models
- Katalogadresse: https://api.fireworks.ai/v1/accounts/fireworks/models
