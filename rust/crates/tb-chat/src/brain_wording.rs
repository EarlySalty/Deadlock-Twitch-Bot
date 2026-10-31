use tb_llm::brain_route::{
    answered_text, ChatLine, ConversationPort, ConversationTask, StreamContext, MAX_CHAT_LINES,
    MAX_CHAT_LINE_CHARS,
};

pub(crate) fn stream_context(streamer_login: &str, deadlock_live: bool) -> StreamContext {
    StreamContext {
        streamer_login: streamer_login.to_string(),
        game: None,
        title: None,
        deadlock_live,
    }
}

pub(crate) fn chat_lines(lines: &[String]) -> Vec<ChatLine> {
    let start = lines.len().saturating_sub(MAX_CHAT_LINES);
    lines[start..]
        .iter()
        .filter_map(|line| chat_line(line))
        .collect()
}

fn chat_line(line: &str) -> Option<ChatLine> {
    let (author, text) = line.split_once(": ")?;
    let (author, text) = (author.trim(), text.trim());
    if author.is_empty() || text.is_empty() {
        return None;
    }
    Some(ChatLine {
        author: author.to_string(),
        text: text.chars().take(MAX_CHAT_LINE_CHARS).collect(),
    })
}

pub(crate) async fn phrase(
    conversation: &dyn ConversationPort,
    broadcaster_id: &str,
    task: ConversationTask,
) -> Option<String> {
    answered_text(conversation.converse(broadcaster_id, task).await.ok()?)
}
