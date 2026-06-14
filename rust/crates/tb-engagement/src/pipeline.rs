//! Engagement-Pipeline — reine Helfer (Slice 1).
//!
//! Port der I/O-freien Logik aus `bot/engagement/pipeline.py`: der billige
//! Pre-Filter ([`should_skip_trigger`]) und die Kostenrechnung
//! ([`calc_cost_usd`]). Der async Orchestrator (`EngagementPipeline.handle`)
//! folgt in späteren Slices, sobald die Provider portiert sind.

use std::collections::HashSet;

/// Billiger Pre-Filter ohne Modell-Call: Nachrichten, auf die ein Zuschauer nie
/// antworten würde, fliegen sofort raus (Python `_should_skip_trigger`).
///
/// (1) führendes `@name`/`!command` → an eine Person/einen Bot gerichtet;
/// (2) ein einzelnes Token ohne `?` → Emote/Reaktionswort (Ein-Wort-Fragen
/// gehen bewusst weiter); (3) dasselbe Token mehrfach → Emote-/Cheer-Spam.
pub fn should_skip_trigger(content: &str) -> bool {
    let text = content.trim();
    if text.is_empty() {
        return true;
    }
    match text.chars().next() {
        Some('@') | Some('!') => return true,
        _ => {}
    }
    let tokens: Vec<&str> = text.split_whitespace().collect();
    if tokens.len() == 1 && !text.ends_with('?') {
        return true;
    }
    if tokens.len() >= 2 && tokens.iter().collect::<HashSet<_>>().len() == 1 {
        return true;
    }
    false
}

/// Geschätzte MiniMax-Kosten in USD (Python `_calc_cost_usd`). Fehlen Token-
/// Zahlen → None. Die Raten kommen aus Env (`MINIMAX_PRICE_INPUT/OUTPUT_PER_1K`);
/// ist eine gesetzte Rate unparsebar, fallen wie in Python BEIDE auf die
/// Defaults zurück.
pub fn calc_cost_usd(prompt_tokens: Option<i64>, completion_tokens: Option<i64>) -> Option<f64> {
    let pt = prompt_tokens?;
    let ct = completion_tokens?;
    let (input_rate, output_rate) = match (
        parse_rate("MINIMAX_PRICE_INPUT_PER_1K", 0.0008),
        parse_rate("MINIMAX_PRICE_OUTPUT_PER_1K", 0.0024),
    ) {
        (Some(i), Some(o)) => (i, o),
        _ => (0.0008, 0.0024),
    };
    Some((pt as f64 / 1000.0) * input_rate + (ct as f64 / 1000.0) * output_rate)
}

/// `float(os.getenv(var, default))`-Semantik: ungesetzt → Default; gesetzt +
/// parsebar → Wert; gesetzt + unparsebar → None (löst den Beide-Defaults-Pfad
/// aus, wie Pythons `except ValueError`).
fn parse_rate(var: &str, default: f64) -> Option<f64> {
    match std::env::var(var) {
        Ok(v) => v.trim().parse::<f64>().ok(),
        Err(_) => Some(default),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_trigger_faelle() {
        assert!(should_skip_trigger("")); // leer
        assert!(should_skip_trigger("   ")); // nur Whitespace
        assert!(should_skip_trigger("@nani hi")); // an Person
        assert!(should_skip_trigger("!title")); // an Bot
        assert!(should_skip_trigger("gg")); // Ein-Wort ohne ?
        assert!(should_skip_trigger("Cheer1 Cheer1 Cheer1")); // wiederholtes Token
        // Durchgelassen:
        assert!(!should_skip_trigger("haze?")); // Ein-Wort-Frage
        assert!(!should_skip_trigger("was haltet ihr von haze")); // Mehrwort
        assert!(!should_skip_trigger("gg wp")); // zwei verschiedene Tokens
    }

    #[test]
    fn cost_none_ohne_tokens() {
        assert_eq!(calc_cost_usd(None, Some(10)), None);
        assert_eq!(calc_cost_usd(Some(10), None), None);
    }

    #[test]
    fn cost_default_raten() {
        // Ohne gesetzte Env: 1000 Input + 1000 Output → 0.0008 + 0.0024 = 0.0032.
        // (Env-frei im Testprozess vorausgesetzt; Defaults greifen.)
        if std::env::var("MINIMAX_PRICE_INPUT_PER_1K").is_err()
            && std::env::var("MINIMAX_PRICE_OUTPUT_PER_1K").is_err()
        {
            let cost = calc_cost_usd(Some(1000), Some(1000)).unwrap();
            assert!((cost - 0.0032).abs() < 1e-9);
        }
    }
}
