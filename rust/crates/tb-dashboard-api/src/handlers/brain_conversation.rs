use serde_json::Value;
use tb_llm::brain_route::{
    DialogRole, DialogTurn, ReplyLanguage, MAX_DIALOG_CHARS, MAX_DIALOG_TURNS, MAX_DOCUMENT_CHARS,
    MAX_TASK_TEXT_CHARS,
};
use tb_llm::Message;

pub(crate) fn reply_language(value: &Value) -> ReplyLanguage {
    match value.get("language").and_then(Value::as_str).map(str::trim) {
        Some(language) if language.eq_ignore_ascii_case("en") => ReplyLanguage::En,
        _ => ReplyLanguage::De,
    }
}

pub(crate) fn bounded_dialog(history: &[Message]) -> Vec<DialogTurn> {
    let mut used = 0;
    let mut turns: Vec<DialogTurn> = history
        .iter()
        .rev()
        .filter_map(dialog_turn)
        .take(MAX_DIALOG_TURNS)
        .take_while(|turn| {
            used += turn.text.chars().count();
            used <= MAX_DIALOG_CHARS
        })
        .collect();
    turns.reverse();
    turns
}

fn dialog_turn(message: &Message) -> Option<DialogTurn> {
    let role = match message.role.as_str() {
        "user" => DialogRole::User,
        "assistant" => DialogRole::Assistant,
        _ => return None,
    };
    let text = message.content.trim();
    (!text.is_empty()).then(|| DialogTurn {
        role,
        text: text.to_string(),
    })
}

pub(crate) fn bounded_document(text: &str) -> String {
    text.chars().take(MAX_DOCUMENT_CHARS).collect()
}

pub(crate) fn bounded_question(text: &str) -> String {
    text.chars().take(MAX_TASK_TEXT_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_behaelt_neueste_beitraege_innerhalb_der_grenzen() {
        let history: Vec<Message> = (0..60)
            .map(|index| Message::user(format!("{index} {}", "x".repeat(1000))))
            .collect();
        let dialog = bounded_dialog(&history);
        let chars: usize = dialog.iter().map(|turn| turn.text.chars().count()).sum();
        assert!(dialog.len() <= MAX_DIALOG_TURNS);
        assert!(chars <= MAX_DIALOG_CHARS);
        assert!(dialog
            .last()
            .is_some_and(|turn| turn.text.starts_with("59 ")));
    }

    #[test]
    fn dialog_verwirft_fremde_rollen_und_leere_beitraege() {
        let history = [
            Message {
                role: "system".into(),
                content: "x".into(),
            },
            Message::assistant("  "),
            Message::assistant("antwort"),
        ];
        let dialog = bounded_dialog(&history);
        assert_eq!(dialog.len(), 1);
        assert_eq!(dialog[0].role, DialogRole::Assistant);
    }
}
