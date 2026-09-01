//! Prozess-isolierte Verifikation des MiniMax-Usage-Ledger-Seiteneffekts.
//!
//! `generate()` und `raw_completion_tracked()` müssen den echten Token-Verbrauch
//! best-effort in die zentrale `public.minimax_usage` schreiben; `raw_completion()`
//! (untracked) darf NICHTS schreiben. Parität zu Pythons `minimax_usage.record(...)`
//! bzw. `_track_minimax_completion(...)`.
//!
//! Warum ein EIGENES Test-Binary statt eines Unit-Tests: der Ledger-Pool von tb-llm
//! ist ein prozessweiter `OnceCell<PgPool>`. Seine PG-Verbindungen sind an das
//! tokio-Runtime des ERSTEN `record()` gebunden. Im Lib-Test-Binary baut jeder
//! `#[tokio::test]` ein eigenes, kurzlebiges Runtime; sobald das Pool-bauende
//! Runtime endet, hängt der nächste Acquire bis zum `acquire_timeout` und Zeilen
//! gehen verloren. In diesem separaten Binary läuft NUR dieser eine Test, sein
//! einziges Runtime baut UND nutzt den Pool und bleibt dabei am Leben → verlässlich.
//!
//! Alle Ledger-Schreiber im Prozess (`source='twitch-bot'`) teilen sich die Tabelle;
//! die Assertions nutzen daher pro Fall eindeutige Token-Zahlen (777/333, 888/444,
//! 999/111). Ohne `TB_TEST_DATABASE_URL`: Skip.

use sqlx::postgres::PgPoolOptions;
use tb_engagement::minimax_chat::{ChatMessage, EngagementMinimaxClient};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client_for(server: &MockServer) -> EngagementMinimaxClient {
    EngagementMinimaxClient::new(
        Some("test-key".to_string()),
        Some(server.uri()),
        Some("MiniMax-M3".to_string()),
        None,
    )
}

fn history() -> Vec<ChatMessage> {
    vec![ChatMessage {
        role: "user".to_string(),
        content: "bebop auf der lane?".to_string(),
        name: Some("chatter1".to_string()),
    }]
}

async fn mock_usage(server: &MockServer, content: &str, prompt: i64, completion: i64) {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"content": content}}],
            "usage": {"prompt_tokens": prompt, "completion_tokens": completion}
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn engagement_client_verbucht_usage_ins_zentrale_ledger() {
    let Ok(dsn) = std::env::var("TB_TEST_DATABASE_URL") else {
        return; // ohne Wegwerf-Test-DB nicht verifizierbar
    };
    let dsn = dsn.trim().to_string();
    if dsn.is_empty() {
        return;
    }
    // Den prozessweiten Ledger-Pool auf die Wegwerf-Test-DB zeigen lassen. Da NUR
    // dieser Test im Binary läuft, baut sein Runtime den Pool und bleibt am Leben.
    std::env::set_var("TWITCH_ANALYTICS_DSN", &dsn);
    std::env::remove_var("DATABASE_URL");

    let verify = PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn)
        .await
        .expect("Test-DB verbinden");

    // 1) generate() → engagement 777/333.
    {
        let server = MockServer::start().await;
        mock_usage(&server, "klar", 777, 333).await;
        client_for(&server)
            .generate("system", &history(), 500, 480)
            .await
            .unwrap();
        let row: (String, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT source, purpose, model FROM public.minimax_usage \
             WHERE tokens_in = 777 AND tokens_out = 333 ORDER BY id DESC LIMIT 1",
        )
        .fetch_one(&verify)
        .await
        .expect("Ledger-Zeile mit 777/333 vorhanden");
        assert_eq!(row.0, "twitch-bot");
        assert_eq!(row.1.as_deref(), Some("engagement"));
        assert_eq!(
            row.2.as_deref(),
            Some(tb_llm::selection::FIREWORKS_DEFAULT_MODEL)
        );
    }

    // 2) raw_completion_tracked() → chat-deep-analysis 888/444.
    {
        let server = MockServer::start().await;
        mock_usage(&server, "tiefe analyse", 888, 444).await;
        let text = client_for(&server)
            .raw_completion_tracked("", "prompt", 5000, 0.1, "chat-deep-analysis")
            .await
            .unwrap();
        assert_eq!(text, "tiefe analyse");
        let row: (String, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT source, purpose, model FROM public.minimax_usage \
             WHERE tokens_in = 888 AND tokens_out = 444 ORDER BY id DESC LIMIT 1",
        )
        .fetch_one(&verify)
        .await
        .expect("Ledger-Zeile mit 888/444 vorhanden");
        assert_eq!(row.0, "twitch-bot");
        assert_eq!(row.1.as_deref(), Some("chat-deep-analysis"));
        assert_eq!(
            row.2.as_deref(),
            Some(tb_llm::selection::FIREWORKS_DEFAULT_MODEL)
        );
    }

    // 3) raw_completion() (untracked) → schreibt KEINE Zeile (999/111 bleibt 0).
    {
        let server = MockServer::start().await;
        mock_usage(&server, "x", 999, 111).await;
        client_for(&server)
            .raw_completion("", "p", 100, 0.4)
            .await
            .unwrap();
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM public.minimax_usage \
             WHERE tokens_in = 999 AND tokens_out = 111",
        )
        .fetch_one(&verify)
        .await
        .expect("Count-Query");
        assert_eq!(count.0, 0, "raw_completion darf nicht ins Ledger schreiben");
    }

    verify.close().await;
}
