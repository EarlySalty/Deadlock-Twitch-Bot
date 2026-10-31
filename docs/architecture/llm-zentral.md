# LLM-Aufrufe: ein zentraler Fireworks-Connector

Stand: 2026-10-01

## Verbindliche Architektur

Alle Chat-Modell-Aufrufe des Twitch-Bots laufen über genau einen Eingang:

```rust
tb_llm::complete(use_case: &str, request: Request) -> Result<Response, LlmError>
```

Der Eingang übernimmt HTTP-Transport, Fristen, 429-Wiederholungen, Token-Ledger,
`<think>`-Bereinigung und ein optionales Akzeptanz-Prädikat. Fachliche Crates
bauen keine eigenen LLM-Clients und lösen keine Anbieterschlüssel selbst auf.

## Freigegebener Anbieter und gemeinsame Auswahl

Fireworks verwendet die täglich zentral geprüfte stabile DeepSeek-Flash-Familie ab V4.1. Pro und andere Anbieter sind nicht freigegeben. Alle Dienste lesen die lokale Auswahl beim nächsten Request. Der einzige tägliche Rust-Writer prüft Katalog und echte synthetische Inferenz vor Veröffentlichung; kein Bot löst Modelle separat auf.

Der vollständige Datei-, Sicherheits- und Betriebsvertrag steht in [flash-modellauswahl.md](flash-modellauswahl.md). Ohne Fireworks-Schlüssel oder gültigen geprüften Auswahl-State schlägt der Connector geschlossen fehl. Die Basisadresse bleibt `https://api.fireworks.ai/inference/v1`; abweichende Adressen sind ausschließlich für lokale Tests möglich.

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

## Self-Explainer, Dashboard-Assistent und Analyse-Chat

Diese drei Oberflächen laufen nicht über `tb_llm::complete`. Sie rufen
`tb_llm::brain_route::converse` mit den Capabilities `twitch_self_explainer`,
`twitch_dashboard_assistant` und `twitch_analysis_chat` auf und senden nur
strukturierte Eingaben: Frage, Verlauf, Wissensausschnitte, Datenkarten oder
Analysepayload. Prompts und Wortlaut liegen im Deadlock Brain.

`Silent` kommt als HTTP 200 mit `status: "silent"` an und wird in der Oberfläche
als neutraler Hinweis gezeigt. Lokal erzeugte Ersatzantworten gibt es nicht.
Brain-Fehler bleiben Fehler. Eine stille Antwort im Analyse-Chat verbraucht
keine Rückfrage.

Der öffentliche Self-Explainer hat keinen eigenen Kanal und läuft deshalb unter
`twitch.bot_user_id`. Die Antwortfrist kommt aus
`dashboard.options.brain_client.timeout_ms`. Datenbank- und Discord-Protokollierung
laufen nachgelagert und blockieren die sichtbare Antwort nicht.

## Bewusst getrennt

Audio-Transkription ist Sprache-zu-Text und kein Chat-Modell. Twitch-, Discord-
und andere fachliche HTTP-Clients gehören ebenfalls nicht in `tb-llm`.
