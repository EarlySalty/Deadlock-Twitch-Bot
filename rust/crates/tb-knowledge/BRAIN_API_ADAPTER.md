# Typisierter Brain-Consumer

Die öffentliche Self-Explainer-Route nutzt diesen Adapter nicht mehr. Sie läuft über `tb_llm::brain_route::converse`, siehe `docs/architecture/llm-zentral.md`. Legacy- und Shadow-Route des Self-Explainers sind entfernt; `dashboard.options.brain_client.mode` wirkt im Dashboard nicht mehr.

`tb_knowledge::brain::BrainKnowledgeAdapter` verwendet ausschließlich den kanonischen `AsyncBrainClient` aus Deadlock-Brain, gepinnt auf `3b86d3cbe5ea39a67b8b1fbd8a3d48ab935982ef`. Der Adapter enthält keinen direkten Provider-Aufruf und keinen lokalen Retrieval-/RAG-Fallback.
