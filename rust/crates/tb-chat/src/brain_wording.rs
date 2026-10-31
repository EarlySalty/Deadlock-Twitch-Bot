use tb_llm::brain_route::{
    answered_text, ChatLine, ConversationPort, ConversationTask, StreamContext, MAX_CHAT_LINES,
    MAX_CHAT_LINE_CHARS, MAX_LABEL_CHARS, MAX_LIST_ITEMS, MAX_LIST_ITEM_CHARS,
};

pub(crate) fn stream_context(streamer_login: &str, deadlock_live: bool) -> StreamContext {
    StreamContext {
        streamer_login: streamer_login.to_string(),
        game: None,
        title: None,
        deadlock_live,
    }
}

pub(crate) fn live_stream(
    streamer_login: &str,
    game: Option<String>,
    title: Option<String>,
) -> StreamContext {
    let deadlock_live = game
        .as_deref()
        .is_some_and(|game| game.eq_ignore_ascii_case("Deadlock"));
    StreamContext {
        streamer_login: streamer_login.to_string(),
        game: game.map(|game| game.chars().take(MAX_LABEL_CHARS).collect()),
        title: title.map(|title| title.chars().take(MAX_LIST_ITEM_CHARS).collect()),
        deadlock_live,
    }
}

pub(crate) fn list_items(items: &[String]) -> Vec<String> {
    items
        .iter()
        .take(MAX_LIST_ITEMS)
        .map(|item| item.chars().take(MAX_LIST_ITEM_CHARS).collect())
        .collect()
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
