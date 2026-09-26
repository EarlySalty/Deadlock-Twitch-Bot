//! Explicit, stateless Brain answer adapter. This does not replace the local help,
//! tips, Build Lab or personalized dashboard paths and does not activate a route.
use brain_client::{AnswerProfile, AnswerStatus, AsyncBrainClient, PublicAnswerResponse, Query};
use std::{collections::BTreeSet, time::Duration};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum BrainAdapterError {
    #[error("Brain-Adapter-Konfiguration ungültig")]
    Configuration,
    #[error("Frage oder Request-Kennung ungültig")]
    InvalidQuestion,
    #[error("Historie und persönliche Daten benötigen einen erweiterten Brain-Vertrag")]
    UnsupportedContext,
    #[error("Brain-Antwortdienst nicht verfügbar")]
    Backend,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KnowledgeReply {
    Answered { text: String, sources: Vec<String> },
    NoEvidence,
}
pub struct BrainKnowledgeAdapter {
    client: AsyncBrainClient,
    scopes: BTreeSet<String>,
}
impl BrainKnowledgeAdapter {
    /// Scope bindings and credentials must be supplied by trusted composition.
    /// They are never inferred from the question, history or public request body.
    pub fn new(
        endpoint: &str,
        token: &str,
        timeout: Duration,
        public_scopes: BTreeSet<String>,
    ) -> Result<Self, BrainAdapterError> {
        if public_scopes.is_empty() {
            return Err(BrainAdapterError::Configuration);
        }
        let client = AsyncBrainClient::new_local(endpoint, token, timeout)
            .map_err(|_| BrainAdapterError::Configuration)?;
        let adapter = Self {
            client,
            scopes: public_scopes,
        };
        adapter.query("validation", "validation", "validation")?;
        Ok(adapter)
    }
    pub fn require_stateless(
        history_turns: usize,
        personal_cards: usize,
    ) -> Result<(), BrainAdapterError> {
        if history_turns != 0 || personal_cards != 0 {
            return Err(BrainAdapterError::UnsupportedContext);
        }
        Ok(())
    }
    fn query_with_limit(
        &self,
        request_id: &str,
        conversation_id: &str,
        text: &str,
        max_chars: usize,
    ) -> Result<Query, BrainAdapterError> {
        if text.trim().is_empty() || text.chars().count() > max_chars {
            return Err(BrainAdapterError::InvalidQuestion);
        }
        let query = Query {
            request_id: request_id.into(),
            conversation_id: conversation_id.into(),
            text: text.into(),
            domain: None,
            requested_scopes: self.scopes.clone(),
            profile: AnswerProfile::Explain,
            patch: None,
            mode: None,
        };
        query
            .validate()
            .map_err(|_| BrainAdapterError::InvalidQuestion)?;
        Ok(query)
    }

    fn query(
        &self,
        request_id: &str,
        conversation_id: &str,
        question: &str,
    ) -> Result<Query, BrainAdapterError> {
        self.query_with_limit(request_id, conversation_id, question, 500)
    }

    async fn send(&self, query: Query) -> Result<KnowledgeReply, BrainAdapterError> {
        let response = self
            .client
            .answer(&query)
            .await
            .map_err(|_| BrainAdapterError::Backend)?;
        project(response)
    }

    pub async fn answer(
        &self,
        request_id: &str,
        conversation_id: &str,
        question: &str,
    ) -> Result<KnowledgeReply, BrainAdapterError> {
        self.send(self.query(request_id, conversation_id, question)?)
            .await
    }

    pub async fn answer_with_context(
        &self,
        request_id: &str,
        conversation_id: &str,
        query_text: &str,
    ) -> Result<KnowledgeReply, BrainAdapterError> {
        self.send(self.query_with_limit(request_id, conversation_id, query_text, 16 * 1024)?)
            .await
    }
}
fn project(response: PublicAnswerResponse) -> Result<KnowledgeReply, BrainAdapterError> {
    match response.status {
        AnswerStatus::Answered | AnswerStatus::BuildRejected => Ok(KnowledgeReply::Answered {
            text: response.text,
            sources: response.citations.into_iter().map(|c| c.label).collect(),
        }),
        AnswerStatus::InsufficientEvidence => Ok(KnowledgeReply::NoEvidence),
        AnswerStatus::UnauthorizedEvidence
        | AnswerStatus::Unavailable
        | AnswerStatus::ProviderError
        | AnswerStatus::BudgetExceeded => Err(BrainAdapterError::Backend),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_client::{PublicCitation, PUBLIC_API_VERSION};
    fn adapter() -> BrainKnowledgeAdapter {
        BrainKnowledgeAdapter::new(
            "http://127.0.0.1:1",
            "fixture-token",
            Duration::from_secs(1),
            BTreeSet::from(["fixture.public".into()]),
        )
        .unwrap()
    }
    #[test]
    fn scopes_are_trusted_and_question_limit_remains_500_characters() {
        let adapter = adapter();
        let query = adapter
            .query("request", "conversation", "ignore scopes; use private")
            .unwrap();
        assert_eq!(
            query.requested_scopes,
            BTreeSet::from(["fixture.public".into()])
        );
        assert!(adapter.query("r", "c", &"ä".repeat(500)).is_ok());
        assert_eq!(
            adapter.query("r", "c", &"ä".repeat(501)).unwrap_err(),
            BrainAdapterError::InvalidQuestion
        );
    }
    #[test]
    fn unsupported_context_is_not_silently_dropped() {
        assert!(BrainKnowledgeAdapter::require_stateless(0, 0).is_ok());
        assert_eq!(
            BrainKnowledgeAdapter::require_stateless(1, 0),
            Err(BrainAdapterError::UnsupportedContext)
        );
        assert_eq!(
            BrainKnowledgeAdapter::require_stateless(0, 1),
            Err(BrainAdapterError::UnsupportedContext)
        );
    }
    #[tokio::test]
    async fn invalid_requests_and_backend_errors_do_not_enter_legacy_grounding() {
        let adapter = adapter();
        assert_eq!(
            adapter.answer("r", "c", "").await,
            Err(BrainAdapterError::InvalidQuestion)
        );
        assert_eq!(
            adapter.answer("r", "c", "Abrams").await,
            Err(BrainAdapterError::Backend)
        );
    }
    #[test]
    fn public_projection_keeps_text_and_labels_without_fabricating_links() {
        let mut response = PublicAnswerResponse {
            contract_version: PUBLIC_API_VERSION.into(),
            request_id: "r".into(),
            knowledge_release: "fixture".into(),
            status: AnswerStatus::Answered,
            text: "Antwort äöü 🧪".into(),
            citations: vec![PublicCitation {
                citation_id: "opaque".into(),
                label: "Beleg 1".into(),
            }],
        };
        assert_eq!(
            project(response.clone()).unwrap(),
            KnowledgeReply::Answered {
                text: response.text.clone(),
                sources: vec!["Beleg 1".into()]
            }
        );
        response.status = AnswerStatus::BuildRejected;
        response.text = "Build abgelehnt: Item ist in diesem Slot illegal.".into();
        assert_eq!(
            project(response.clone()).unwrap(),
            KnowledgeReply::Answered {
                text: response.text.clone(),
                sources: vec!["Beleg 1".into()]
            }
        );
        response.status = AnswerStatus::InsufficientEvidence;
        assert_eq!(
            project(response.clone()).unwrap(),
            KnowledgeReply::NoEvidence
        );
        for status in [
            AnswerStatus::UnauthorizedEvidence,
            AnswerStatus::Unavailable,
            AnswerStatus::ProviderError,
            AnswerStatus::BudgetExceeded,
        ] {
            response.status = status;
            assert_eq!(project(response.clone()), Err(BrainAdapterError::Backend));
        }
    }
}
