# LLM-Aufrufe: ein zentraler Fireworks-Connector

Stand: 2026-09-01

## Verbindliche Architektur

Alle Chat-Modell-Aufrufe des Twitch-Bots laufen über genau einen Eingang:

```rust
tb_llm::complete(use_case: &str, request: Request) -> Result<Response, LlmError>
```

Der Eingang übernimmt HTTP-Transport, Fristen, 429-Wiederholungen, Token-Ledger,
`<think>`-Bereinigung und ein optionales Akzeptanz-Prädikat. Fachliche Crates
bauen keine eigenen LLM-Clients und lösen keine Anbieterschlüssel selbst auf.

## Freigegebener Anbieter und Modell-Lock

- Anbieter: Fireworks
- Basisadresse: `https://api.fireworks.ai/inference/v1`
- Modell: `accounts/fireworks/models/deepseek-v4-flash-0731`
- Schlüssel: `FIREWORK_API_KEY`, kompatibler Alias `FIREWORKS_API_KEY`

Anbieter- und Modell-Overrides sind abgeschaltet. Variablen früherer Anbieter
werden nicht ausgewertet. Ohne Fireworks-Schlüssel schlägt
der Connector geschlossen fehl; es gibt keinen Rückfall auf einen anderen
Anbieter. Eine abweichende Basisadresse ist nur für lokale Mock- und
Proxy-Tests möglich, der Modellname bleibt dabei fest.

## Request-Vertrag

`Request` trägt System-Prompt, Verlauf, Token-Limit, Temperatur, Antwortformat,
Zeitgrenze und Ledger-Zweck. `endpoint` bleibt für Tests und fachlich
festgenagelte Fireworks-Pfade erhalten. Produktive Aufrufer beziehen Adresse,
Modell und Schlüssel über `tb_llm::endpoint_for` oder rufen direkt
`tb_llm::complete` mit einem Use-Case auf.

`endpoint_chain` enthält höchstens einen Fireworks-Endpunkt. Ohne Schlüssel ist
die Kette leer. Ein gesetzter Altanbieter-Schlüssel darf die Kette niemals
füllen.

## Anwendungsfälle

Unter anderem laufen folgende Pfade über den zentralen Eingang:

- Engagement- und Folgechat
- Dashboard-KI und Self-Explainer
- Titel-KI und Spam-Judge
- Post-Stream-Auswertung
- Social-Media-Anreicherung
- Schatten-Reviews und Stream-Audit

Die Use-Case-Namen trennen Logs und Ledger-Zwecke, nicht Anbieter oder Modelle.
Die vorhandene Einwilligung für externe Social-Media-Anreicherung bleibt ein
Pflicht-Gate; ohne Einwilligung wird nicht auf ein lokales Modell ausgewichen.

## Self-Explainer

Der öffentliche Self-Explainer nutzt den Use-Case
`dashboard_self_explainer` direkt über `tb_llm::complete`. Er erhält die letzten
acht Gesprächsbeiträge, damit Folgeaussagen wie „das hat mich abgehalten“ auf
die vorher besprochenen Berechtigungen bezogen werden können.

Das Modell hat bis zu 110 Sekunden. Der Handler antwortet spätestens nach 115
Sekunden und liefert bei einem Modellfehler oder einer Zeitüberschreitung für
dokumentierte Berechtigungsthemen eine grounded Ersatzantwort. Browser brechen
erst nach 125 Sekunden ab. Datenbank- und Discord-Protokollierung laufen danach
und blockieren die sichtbare Antwort nicht.

## Bewusst getrennt

Audio-Transkription ist Sprache-zu-Text und kein Chat-Modell. Twitch-, Discord-
und andere fachliche HTTP-Clients gehören ebenfalls nicht in `tb-llm`.
