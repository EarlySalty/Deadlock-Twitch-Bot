# Typisierter Brain-Consumer für Twitch/Self-Explainer

Stand: 2026-09-25. Vorbereiteter Opt-in-Port, **keine aktive Route umgestellt**.

`tb_knowledge::brain::BrainKnowledgeAdapter` verwendet ausschließlich den kanonischen Async-Client aus Deadlock-Brain, gepinnt auf `bdcc6dec3424bd313d36e5f545de2a07df564c7f` (Brain PR #39, auf CODEX A/PR #38). Kein direkter Provider-Aufruf, kein neuer lokaler Retrieval-/RAG-Fallback und kein automatisches Laden von Runtime-Konfiguration.

## Vorbereitung und bestehende Semantik

Der spätere Composition Root liefert Loopback-Endpunkt, Bearer-Token, Timeout und öffentliche Scope-Bindungen. Request- und Conversation-ID müssen vertrauenswürdig dem tatsächlichen Aufruf zugeordnet werden. Scope- oder Principal-Vorgaben aus Chat-/Fragetexten werden nicht übernommen; die echte Autorität bleibt beim serverseitig gebundenen Token.

Der neue, derzeit unbenutzte Handler-Port `self_explainer::answer_stateless_via_brain` übernimmt die bestehende Fragebegrenzung und Antwortaufbereitung: maximal 500 Unicode-Zeichen pro Frage, bisheriger Output-Filter, 2000-Zeichen-Antwortlimit und Injection-Markierung. Der äußere Handler, seine Rate-Limits, 400-Zeichen-Aufteilung und JSON-Form bleiben unverändert. Opake Quellenlabels bleiben Labels; nicht gelieferte Quellen-URLs werden nicht erfunden. Fehlende Evidenz wird als dokumentierte fehlende Antwort behandelt; Transport-/Provider-/ACL-Fehler erzeugen keinen zweiten Antwortpfad.

## Kein stiller Featureverlust

`require_stateless` weist vorhandene Historie oder persönliche Datenkarten ausdrücklich zurück. Das aktuelle öffentliche Brain-API modelliert Historie, Dashboard-Karten, Persona/Sprache, Concierge-/Aktionsmetadaten und Build-Publishing nicht vollständig. Ein bloßer Austausch des Clients würde hier Verhalten verlieren oder unsicheres Kontext-Prompting einführen.

Deshalb bleiben personalisierter Dashboard-Assistent, bisherige Self-Explainer-Route mit Historie, lokale Hilfeseite/Tipps und Build-Lab-Pfade unberührt. Für einen vollständigen Cutover dieser Pfade ist vor der Runtime-Verbindung weitere **Vertrags-/Codearbeit** erforderlich. Der neue stateless Port behauptet keine Featureparität für nicht unterstützte Modi.

## Offline-Prüfung

```sh
cargo fetch --manifest-path rust/Cargo.toml --locked
bash rust/scripts/check-brain-consumer.sh knowledge
bash rust/scripts/check-brain-consumer.sh self-explainer
```

Rust 1.97.1, rustfmt/clippy, SQLX_OFFLINE. Tests erhalten ein eigenes HOME und keine geerbte App-Konfiguration. Die GitHub-Actions-Matrix prüft beide Bereiche unabhängig, damit ein Fehler im Corpus-Regressionstest nicht die Prüfung des neuen Handler-Ports verhindert. Alle Fehler bleiben rot; kein `continue-on-error`, kein Herausfiltern des bekannten Seed-Tests.

### Nachgewiesener Altfehler

`tb-knowledge/tests/seed.rs::stoerung_stream_info_felder_findet_uplink_stoerungen` scheitert bereits auf dem unveränderten Basis-Commit `8c800bb9`: Der erwartete Slug ist nicht unter den fünf Treffern. Gegenprobe im separaten, unveränderten Baseline-Worktree: 4 Seed-Tests bestanden, derselbe eine fehlgeschlagen. Auf dem Adapter-Branch: 23 Unit-Tests und 3 Load-Tests bestanden, Seed ebenfalls 4 bestanden / 1 fehlgeschlagen. Selector und Wissensbestand wurden für den Adapter nicht verändert. Dieser Altfehler ist ausdrücklich kein grüner Regressionstest.

Die gezielte Self-Explainer-Suite besteht mit 19 Tests einschließlich des neuen typed-port-Tests. Sie ist nicht gleichbedeutend mit der vollständigen Dashboard-Suite; deren übrige Tests sind bei diesem Aufruf gefiltert. Ein bestehender Deprecation-Hinweis in `uplink_config.rs` ist separat vom Adapter.

## Claude-Handoff

Öffentlichen Token-Prinzipal und Scope-ACL prüfen, Request-/Conversation-Bindung im aufrufenden Handler ergänzen und erst dann den stateless Port in isolierter Runtime verbinden. Auth, Rate-Limits, Ausgabeaufteilung, Quellenlabels, Providerfehler und Timeouts gegen echte Runtime verifizieren. Vor weiteren Modi die oben genannten Vertragslücken schließen und den nachgewiesenen Corpus-Altfehler bearbeiten. Keine Produktionskonfiguration geändert, kein Bot gestartet, keine Nachricht oder Deployment-Aktion ausgeführt.
