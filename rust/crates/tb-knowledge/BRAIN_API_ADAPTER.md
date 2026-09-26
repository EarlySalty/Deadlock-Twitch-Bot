# Typisierter Brain-Consumer für Twitch/Self-Explainer

Stand: 2026-09-26. C9 verdrahtet den typisierten Brain-Port in die echte öffentliche Self-Explainer-Route, ohne den bisherigen Pfad produktiv abzuschalten.

`tb_knowledge::brain::BrainKnowledgeAdapter` verwendet ausschließlich den kanonischen `AsyncBrainClient` aus Deadlock-Brain, gepinnt auf `3b86d3cbe5ea39a67b8b1fbd8a3d48ab935982ef`. Der Adapter enthält keinen direkten Provider-Aufruf und keinen lokalen Retrieval-/RAG-Fallback.

## Runtime-Modi

Die bestehende Route `POST /twitch/api/v2/self-explainer/ask` installiert `SelfExplainerBrainRuntime` im Router. Der Default bleibt `legacy`. Eine Umstellung erfolgt nur durch explizite Runtime-Konfiguration:

- `TWITCH_BRAIN_CLIENT_MODE=legacy|shadow|typed`
- `TWITCH_BRAIN_API_ENDPOINT` — vom BrainClient auf Loopback begrenzt
- `TWITCH_BRAIN_API_TOKEN` — Bearer-Credential
- `TWITCH_BRAIN_API_SCOPES` — vertrauenswürdige, kommagetrennte Scope-Bindung
- `TWITCH_BRAIN_API_TIMEOUT_MS` — Default 8000 ms

`typed` verwendet für die sichtbare Antwort ausschließlich brain-serve. Ein Transport-/ACL-/Provider-/`unavailable`-Fehler fällt nicht still auf das alte Modell/RAG zurück, sondern nutzt nur die bestehende sichere Unsicherheitsantwort. `shadow` ruft den typisierten Port zusätzlich report-only auf und lässt weiterhin den Legacy-Pfad sichtbar antworten.

## Historie und Ausgabe

Die bestehende, bereits begrenzte Historie wird im typed/shadow-Pfad nicht verworfen: maximal acht Turns mit maximal 500 Zeichen pro Turn werden als ausdrücklich *untrusted conversation context* an den typisierten Query-Text angehängt. Scopes oder Autorität werden daraus nie abgeleitet.

Das bestehende Ausgabeformat bleibt erhalten: Antwortfilter, 2000-Zeichen-Grenze, 400-Zeichen-`parts`, `grounded`, Quellenlabels, Injection-Markierung sowie die nachgelagerten Logging-Wege bleiben auf Route-Ebene gleich.

Statusabbildung:

- `answered` und `build_rejected` → sichtbarer Antworttext
- `insufficient_evidence` → bestehende No-Evidence-Antwort
- `unavailable`, `unauthorized_evidence`, `provider_error`, `budget_exceeded` → Backendfehler im Adapter; kein zweiter Antwortpfad

## Prüfung

C9 prüft den Adapter und den echten Router-Composition-Punkt mit Rust 1.97.1. Eine isolierte echte brain-serve-Instanz mit Test-Credential/ACL bleibt als lokaler Integrationscheck vor einer späteren Aktivierung nötig.

Keine Produktionskonfiguration wurde geändert, kein Twitch-Bot gestartet, keine Nachricht gesendet und kein Deployment ausgeführt.
