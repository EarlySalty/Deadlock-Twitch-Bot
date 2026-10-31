use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tb_llm::brain_route::{BotReply, ConversationPort, ConversationTask, SilentReason};
use tb_llm::LlmError;

pub(crate) struct ScriptedBrain {
    pub reply: Result<BotReply, String>,
    pub tasks: Mutex<Vec<ConversationTask>>,
}

#[async_trait]
impl ConversationPort for ScriptedBrain {
    async fn converse(
        &self,
        _broadcaster_id: &str,
        task: ConversationTask,
    ) -> Result<BotReply, LlmError> {
        self.tasks.lock().expect("tasks lock").push(task);
        self.reply.clone().map_err(LlmError::Unavailable)
    }
}

pub(crate) fn scripted(reply: Result<BotReply, String>) -> Arc<ScriptedBrain> {
    Arc::new(ScriptedBrain {
        reply,
        tasks: Mutex::new(Vec::new()),
    })
}

pub(crate) fn answering(text: &str) -> Arc<ScriptedBrain> {
    scripted(Ok(BotReply::Answered {
        text: text.to_string(),
        signals: Vec::new(),
        citations: Vec::new(),
        artifacts: Vec::new(),
    }))
}

pub(crate) struct EchoBrain;

#[async_trait]
impl ConversationPort for EchoBrain {
    async fn converse(
        &self,
        _broadcaster_id: &str,
        task: ConversationTask,
    ) -> Result<BotReply, LlmError> {
        let (author, invite_url) = match task {
            ConversationTask::TwitchInviteReply {
                author, invite_url, ..
            }
            | ConversationTask::TwitchLfgReply {
                author, invite_url, ..
            } => (author, invite_url),
            _ => return Err(LlmError::Unavailable("nicht erwartet".to_string())),
        };
        Ok(BotReply::Answered {
            text: echo_text(&author, invite_url.as_deref().unwrap_or_default()),
            signals: Vec::new(),
            citations: Vec::new(),
            artifacts: Vec::new(),
        })
    }
}

pub(crate) fn echo_text(author: &str, invite_url: &str) -> String {
    format!("@{author} Brain {invite_url}")
}

pub(crate) fn silent() -> Arc<ScriptedBrain> {
    scripted(Ok(BotReply::Silent {
        reason: SilentReason::NoOccasion,
    }))
}

pub(crate) fn failing() -> Arc<ScriptedBrain> {
    scripted(Err("Brain nicht erreichbar".to_string()))
}
