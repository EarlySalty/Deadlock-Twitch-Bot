use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

const API_CONTRACT: &str = "brain.public.v1";

#[derive(Clone, Debug)]
pub struct BrainRetrieveConfig {
    pub endpoint: String,
    pub expected_release: String,
    pub deadline: Duration,
    pub max_response_bytes: usize,
    pub max_evidence_count: usize,
    pub max_evidence_bytes: usize,
    pub allowed_game_terms: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrainStatus {
    CandidateEvidence,
    NoRelevantContext,
    Unavailable,
    AuthenticationFailed,
    Incomplete,
}

#[derive(Clone, Debug)]
pub struct BrainEvidence {
    pub citation_id: String,
    pub label: String,
    pub text: String,
    pub kind: String,
}

#[derive(Clone, Debug)]
pub struct BrainRetrieval {
    pub status: BrainStatus,
    pub knowledge_release: Option<String>,
    pub http_status: Option<u16>,
    pub evidence: Vec<BrainEvidence>,
    pub elapsed: Duration,
}

#[derive(Serialize)]
struct RetrieveRequest<'a> {
    request_id: &'a str,
    conversation_id: &'a str,
    text: String,
    requested_scopes: [&'static str; 1],
    profile: &'static str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetrieveResponse {
    contract_version: String,
    request_id: String,
    knowledge_release: String,
    status: String,
    evidence: Vec<EvidenceResponse>,
    truncated: bool,
    out_of_domain: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceResponse {
    citation_id: String,
    label: String,
    text: String,
    kind: String,
}

impl BrainRetrieveConfig {
    pub fn validate(&self) -> bool {
        let Ok(url) = reqwest::Url::parse(&self.endpoint) else {
            return false;
        };
        let host_is_loopback = match url.host() {
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            _ => false,
        };
        url.scheme() == "http"
            && host_is_loopback
            && url.path() == "/v1/retrieve"
            && url.port().is_some_and(|port| port != 0)
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && self.expected_release.len() <= 128
            && !self.expected_release.is_empty()
            && self.expected_release != "current"
            && self
                .expected_release
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
            && !self.deadline.is_zero()
            && self.deadline <= Duration::from_secs(60)
            && (512..=128 * 1024).contains(&self.max_response_bytes)
            && (1..=8).contains(&self.max_evidence_count)
            && (64..=8 * 1024).contains(&self.max_evidence_bytes)
            && !self.allowed_game_terms.is_empty()
            && self.allowed_game_terms.len() <= 4096
            && self.allowed_game_terms.iter().all(|term| valid_term(term))
    }
}

pub async fn retrieve_game_context(
    config: &BrainRetrieveConfig,
    bearer_token: &str,
    target_term: &str,
) -> BrainRetrieval {
    let started = Instant::now();
    let mut outcome = BrainRetrieval {
        status: BrainStatus::Unavailable,
        knowledge_release: None,
        http_status: None,
        evidence: Vec::new(),
        elapsed: Duration::ZERO,
    };
    if !config.validate()
        || bearer_token.trim().is_empty()
        || bearer_token.chars().any(char::is_control)
        || !valid_term(target_term)
        || !config
            .allowed_game_terms
            .iter()
            .any(|term| term.eq_ignore_ascii_case(target_term.trim()))
    {
        outcome.status = BrainStatus::Incomplete;
        outcome.elapsed = started.elapsed();
        return outcome;
    }
    let request_id = format!("social-ai-{}", tb_crypto::random_hex_token(12));
    let conversation_id = format!("social-ai-{}", tb_crypto::random_hex_token(12));
    let request = RetrieveRequest {
        request_id: &request_id,
        conversation_id: &conversation_id,
        text: format!("Deadlock: {}", target_term.trim()),
        requested_scopes: ["bot.public"],
        profile: "explain",
    };
    let Ok(client) = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(config.deadline)
        .build()
    else {
        outcome.elapsed = started.elapsed();
        return outcome;
    };
    let response = client
        .post(&config.endpoint)
        .bearer_auth(bearer_token)
        .json(&request)
        .send()
        .await;
    let Ok(mut response) = response else {
        outcome.elapsed = started.elapsed();
        return outcome;
    };
    outcome.http_status = Some(response.status().as_u16());
    if response.status() == reqwest::StatusCode::UNAUTHORIZED
        || response.status() == reqwest::StatusCode::FORBIDDEN
    {
        outcome.status = BrainStatus::AuthenticationFailed;
        outcome.elapsed = started.elapsed();
        return outcome;
    }
    if response.status() != reqwest::StatusCode::OK {
        outcome.elapsed = started.elapsed();
        return outcome;
    }
    if response
        .content_length()
        .is_some_and(|bytes| bytes > config.max_response_bytes as u64)
    {
        outcome.status = BrainStatus::Incomplete;
        outcome.elapsed = started.elapsed();
        return outcome;
    }
    let mut bytes = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if chunk.len() > config.max_response_bytes.saturating_sub(bytes.len()) {
                    outcome.status = BrainStatus::Incomplete;
                    outcome.elapsed = started.elapsed();
                    return outcome;
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(_) => {
                outcome.elapsed = started.elapsed();
                return outcome;
            }
        }
    }
    outcome.status = BrainStatus::Incomplete;
    if let Ok(body) = serde_json::from_slice::<RetrieveResponse>(&bytes) {
        if body.contract_version == API_CONTRACT
            && body.request_id == request_id
            && body.knowledge_release == config.expected_release
            && !body.truncated
            && !body.out_of_domain
            && body.evidence.len() <= config.max_evidence_count
            && body.evidence.iter().all(|item| {
                item.text.len() <= config.max_evidence_bytes
                    && !item.citation_id.is_empty()
                    && item.citation_id.len() <= 128
                    && !item.label.is_empty()
                    && item.label.len() <= 256
                    && matches!(
                        item.kind.as_str(),
                        "fact" | "rule" | "prose" | "mechanic" | "population" | "replay"
                    )
            })
        {
            outcome.knowledge_release = Some(body.knowledge_release);
            let distinct = body
                .evidence
                .iter()
                .map(|item| item.citation_id.as_str())
                .collect::<HashSet<_>>()
                .len()
                == body.evidence.len();
            if distinct && body.status == "insufficient_evidence" && body.evidence.is_empty() {
                outcome.status = BrainStatus::NoRelevantContext;
            } else if distinct && body.status == "answered" && !body.evidence.is_empty() {
                outcome.evidence = body
                    .evidence
                    .into_iter()
                    .filter(|item| term_in_evidence(&item.text, target_term))
                    .map(|item| BrainEvidence {
                        citation_id: item.citation_id,
                        label: item.label,
                        text: item.text,
                        kind: item.kind,
                    })
                    .collect();
                outcome.status = if outcome.evidence.is_empty() {
                    BrainStatus::NoRelevantContext
                } else {
                    BrainStatus::CandidateEvidence
                };
            }
        }
    }
    outcome.elapsed = started.elapsed();
    outcome
}

fn valid_term(term: &str) -> bool {
    let trimmed = term.trim();
    (2..=64).contains(&trimmed.len())
        && trimmed.bytes().any(|byte| byte.is_ascii_alphabetic())
        && trimmed.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || byte == b' ' || byte == b'-' || byte == b'\''
        })
        && trimmed.split_whitespace().count() <= 4
}

fn term_in_evidence(text: &str, term: &str) -> bool {
    let term = term.trim().to_ascii_lowercase();
    let text = text.to_ascii_lowercase();
    text.match_indices(&term).any(|(index, _)| {
        let before = text[..index].chars().next_back();
        let after = text[index + term.len()..].chars().next();
        !before.is_some_and(|character| character.is_alphanumeric())
            && !after.is_some_and(|character| character.is_alphanumeric())
            && text.split_whitespace().count() >= 5
            && text.len() >= term.len() + 24
    })
}
