use super::*;
use std::sync::Arc;
use wiremock::{
    matchers::{body_partial_json, method, path, query_param},
    Mock, MockServer, ResponseTemplate,
};

fn policy() -> ModelPolicy {
    let mut policy =
        ModelPolicy::from_yaml(include_str!("../../../../knowledge/llm.yaml")).unwrap();
    // Katalogtests nutzen einen ausdrücklich synthetischen Latest-Vertrag.
    policy.selection = "latest".to_string();
    policy.bootstrap_model = model("v4p1-flash");
    policy
}

#[tokio::test]
async fn shipped_policy_preserves_live_model_without_catalog_or_probe() {
    let server = MockServer::start().await;
    let policy = ModelPolicy::from_yaml(include_str!("../../../../knowledge/llm.yaml")).unwrap();
    let resolver = ModelResolver::with_urls(policy, &server.uri(), &server.uri()).unwrap();
    let selected = resolver.resolve("synthetic-key", None, None).await.unwrap();
    assert_eq!(selected, "accounts/fireworks/models/deepseek-v4-flash-0731");
    assert!(resolver
        .resolve("synthetic-key", Some(&selected), None)
        .await
        .is_err());
    assert!(server.received_requests().await.unwrap().is_empty());
}
fn model(version: &str) -> String {
    format!("accounts/fireworks/models/deepseek-{version}")
}
fn entry(version: &str, created: Option<i64>) -> ModelEntry {
    ModelEntry {
        id: model(version),
        created,
    }
}
fn reply(text: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(serde_json::json!({
        "choices": [{"message": {"content": text}}]
    }))
}
fn catalog(versions: &[&str]) -> serde_json::Value {
    serde_json::json!({"models": versions.iter().map(|version| serde_json::json!({
        "name": model(version), "state": "READY", "supportsServerless": true
    })).collect::<Vec<_>>()})
}
fn resolver(server: &MockServer) -> ModelResolver {
    ModelResolver::with_urls(policy(), &format!("{}/models", server.uri()), &server.uri()).unwrap()
}
async fn successful_probe(server: &MockServer, version: &str, count: u64) {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_partial_json(
            serde_json::json!({"model": model(version), "max_tokens": 32}),
        ))
        .respond_with(reply("{\"ready\":true}"))
        .expect(count)
        .mount(server)
        .await;
}

#[test]
fn yaml_ist_die_einzige_versionsquelle() {
    let policy = ModelPolicy::from_yaml(include_str!("../../../../knowledge/llm.yaml")).unwrap();
    assert_eq!(policy.bootstrap_model, model("v4-flash-0731"));
    assert_eq!(policy.selection, "pinned");
    assert!(!policy.allows(&model("v5-flash")));
    assert!(policy.allows(&model("v4-flash-0731")));
    assert!(ModelPolicy::load(Path::new("/nicht-vorhanden/llm.yaml")).is_err());
}

#[test]
fn datierte_mindestversion_schliesst_aeltere_revisionen_aus() {
    let mut dated = policy();
    dated.bootstrap_model = model("v4p1-flash-20260910");
    assert!(!dated.allows(&model("v4p1-flash-20260731")));
    assert!(!dated.allows(&model("v4p1-flash")));
    assert!(dated.allows(&model("v4p1-flash-20260920")));
    assert!(dated.allows(&model("v4p2-flash-20260731")));
}

#[test]
fn kaputte_und_fremde_yaml_schlaegt_geschlossen_fehl() {
    let raw = include_str!("../../../../knowledge/llm.yaml");
    for invalid in [
        raw.replace("fireworks", "fremdanbieter"),
        raw.replace("selection: pinned", "selection: unknown"),
        raw.replace("deepseek-v4-flash", "deepseek-v4-pro"),
        raw.replace("retry_seconds: 60", "retry_seconds: 0"),
        raw.replace("schema_version: 1", "schema_version: 2"),
        format!("{raw}\napi_key: niemals-ausgeben\n"),
        format!("{raw}\nprovider: fremdanbieter\n"),
    ] {
        let error = ModelPolicy::from_yaml(&invalid).unwrap_err();
        assert!(!error.to_string().contains("niemals-ausgeben"));
    }
}

#[test]
fn zahlenversionen_und_fehlende_zeitstempel_sortieren_richtig() {
    let mut entries = vec![
        entry("v4p1-flash", Some(9_000)),
        entry("v4p9-flash", Some(10_000)),
        entry("v4p10-flash", None),
        entry("v10-flash", None),
    ];
    newest_first(&mut entries, &policy());
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        vec![
            model("v10-flash"),
            model("v4p10-flash"),
            model("v4p9-flash"),
            model("v4p1-flash")
        ]
    );
}

#[test]
fn datierte_neufassung_ohne_created_gewinnt_gegen_alte() {
    let mut entries = vec![
        entry("v4p1-flash-0910", Some(1_789_027_200)),
        entry("v4p1-flash-0920", None),
    ];
    newest_first(&mut entries, &policy());
    assert_eq!(entries[0].id, model("v4p1-flash-0920"));
}

#[test]
fn fremde_varianten_und_ungueltige_datumswerte_sind_ausgeschlossen() {
    for version in [
        "v5-pro",
        "v5-flash-preview",
        "v5-flash-lite",
        "v5-flash-thinking",
        "v5-flash-vision",
        "v5-flash-9999",
        "v5-flash-20260230",
        "v5x1-flash",
        "v5p-flash",
    ] {
        assert!(model_version(&model(version)).is_none(), "{version}");
    }
    assert_eq!(
        model_version(&model("v4.1-flash")).unwrap().parts,
        vec![4, 1]
    );
    assert!(!policy().allows("accounts/anderer/models/deepseek-v99-flash"));
}

#[test]
fn cache_ist_an_die_komplette_policy_gebunden() {
    let a = policy();
    let mut b = a.clone();
    b.bootstrap_model = model("v4p2-flash");
    assert_ne!(a.cache_family(), b.cache_family());
    b = a.clone();
    b.max_stale_seconds += 1;
    assert_ne!(a.cache_family(), b.cache_family());
}

#[tokio::test]
async fn paginierung_und_numerisch_neuestes_modell_mit_probe() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .and(query_param("pageToken", "zweite"))
        .respond_with(ResponseTemplate::new(200).set_body_json(catalog(&["v4p10-flash"])))
        .expect(1)
        .with_priority(1)
        .mount(&server)
        .await;
    let mut first = catalog(&["v4p9-flash", "v4p1-flash", "v99-pro"]);
    first["nextPageToken"] = serde_json::json!("zweite");
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(first))
        .expect(1)
        .with_priority(2)
        .mount(&server)
        .await;
    successful_probe(&server, "v4p10-flash", 1).await;
    let resolver = resolver(&server);
    assert_eq!(
        resolver.resolve("synthetic-key", None, None).await.unwrap(),
        model("v4p10-flash")
    );
    assert_eq!(resolver.selected_model(), Some(model("v4p10-flash")));
}

#[tokio::test]
async fn abgelehntes_aktuelles_modell_wird_nicht_bei_spaeterem_probe_503_zurueckgegeben() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(catalog(&["v4p2-flash"])))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_partial_json(
            serde_json::json!({"model": model("v4p2-flash")}),
        ))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_partial_json(
            serde_json::json!({"model": model("v4p1-flash")}),
        ))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&server)
        .await;

    let resolver = resolver(&server);
    *resolver.current.write().unwrap() = Some(VerifiedModel {
        model: model("v4p2-flash"),
        verified_at: Utc::now().timestamp(),
    });
    assert!(resolver.resolve("synthetic-key", None, None).await.is_err());
}

#[tokio::test]
async fn parallele_aufrufer_teilen_katalog_und_probe() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(catalog(&["v4p2-flash"])))
        .expect(1)
        .mount(&server)
        .await;
    successful_probe(&server, "v4p2-flash", 1).await;
    let resolver = Arc::new(resolver(&server));
    let mut handles = Vec::new();
    for _ in 0..12 {
        let resolver = Arc::clone(&resolver);
        handles.push(tokio::spawn(async move {
            resolver.resolve("synthetic-key", None, None).await
        }));
    }
    for handle in handles {
        assert_eq!(handle.await.unwrap().unwrap(), model("v4p2-flash"));
    }
}

#[tokio::test]
async fn gelistetes_totes_modell_wird_nicht_in_cache_uebernommen() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(catalog(&["v4p2-flash", "v4p1-flash"])),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(body_partial_json(
            serde_json::json!({"model": model("v4p2-flash")}),
        ))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;
    successful_probe(&server, "v4p1-flash", 1).await;
    let resolver = resolver(&server);
    assert_eq!(
        resolver.resolve("synthetic-key", None, None).await.unwrap(),
        model("v4p1-flash")
    );
}

#[tokio::test]
async fn katalogausfall_prueft_yaml_startkandidat_ohne_alten_rust_default() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&server)
        .await;
    successful_probe(&server, "v4p1-flash", 1).await;
    assert_eq!(
        resolver(&server)
            .resolve("synthetic-key", None, None)
            .await
            .unwrap(),
        model("v4p1-flash")
    );
}

#[tokio::test]
async fn kaputte_probe_bleibt_fehler_und_verursacht_keinen_anfragesturm() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(catalog(&[])))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(reply("kein JSON"))
        .expect(1)
        .mount(&server)
        .await;
    let resolver = resolver(&server);
    for _ in 0..5 {
        assert!(resolver.resolve("synthetic-key", None, None).await.is_err());
    }
    assert!(resolver.selected_model().is_none());
}

#[tokio::test]
async fn abgelehntes_modell_wird_bei_katalogausfall_nicht_wiederverwendet() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&server)
        .await;
    let resolver = resolver(&server);
    *resolver.current.write().unwrap() = Some(VerifiedModel {
        model: model("v4p1-flash"),
        verified_at: Utc::now().timestamp(),
    });
    assert!(resolver
        .resolve("synthetic-key", Some(&model("v4p1-flash")), None)
        .await
        .is_err());
    assert!(server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .all(|request| request.method == "GET"));
}

#[tokio::test]
async fn abgelaufener_cache_darf_keine_kaputte_probe_ueberstimmen() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let resolver = resolver(&server);
    *resolver.current.write().unwrap() = Some(VerifiedModel {
        model: model("v4p2-flash"),
        verified_at: Utc::now().timestamp() - 200_000,
    });
    assert!(resolver.resolve("synthetic-key", None, None).await.is_err());
}

#[tokio::test]
async fn transienter_refresh_stuft_geprueftes_modell_nicht_zurueck() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(catalog(&["v4p2-flash", "v4p1-flash"])),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(body_partial_json(
            serde_json::json!({"model": model("v4p2-flash")}),
        ))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&server)
        .await;
    successful_probe(&server, "v4p1-flash", 0).await;
    let resolver = resolver(&server);
    *resolver.current.write().unwrap() = Some(VerifiedModel {
        model: model("v4p2-flash"),
        verified_at: Utc::now().timestamp(),
    });
    assert_eq!(
        resolver.resolve("synthetic-key", None, None).await.unwrap(),
        model("v4p2-flash")
    );
}

#[tokio::test]
async fn nicht_bereite_oder_nicht_serverlose_modelle_werden_ignoriert() {
    let server = MockServer::start().await;
    let mut body = catalog(&["v4p2-flash", "v4p3-flash", "v4p1-flash"]);
    body["models"][0]["state"] = serde_json::json!("FAILED");
    body["models"][1]["supportsServerless"] = serde_json::json!(false);
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(&server)
        .await;
    successful_probe(&server, "v4p1-flash", 1).await;
    assert_eq!(
        resolver(&server)
            .resolve("synthetic-key", None, None)
            .await
            .unwrap(),
        model("v4p1-flash")
    );
}

#[tokio::test]
async fn persistenter_cache_wird_nach_neustart_erneut_geprueft_und_alter_begrenzt() {
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    let Ok(dsn) = std::env::var("TB_TEST_DATABASE_URL") else {
        assert_ne!(
            std::env::var("TB_TEST_REQUIRE_DB").as_deref(),
            Ok("1"),
            "TB_TEST_DATABASE_URL fehlt"
        );
        return;
    };
    let schema = format!("t_flash_resolver_{}", std::process::id());
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn)
        .await
        .unwrap();
    assert!(schema
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'));
    sqlx::QueryBuilder::<sqlx::Postgres>::new("CREATE SCHEMA ")
        .push(&schema)
        .build()
        .execute(&admin)
        .await
        .unwrap();
    let options = PgConnectOptions::from_str(&dsn)
        .unwrap()
        .options([("search_path", schema.as_str())]);
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::query(
        "CREATE TABLE llm_model_cache (provider TEXT NOT NULL, family TEXT NOT NULL, \
        model TEXT NOT NULL, model_created BIGINT, resolved_at TIMESTAMPTZ NOT NULL DEFAULT now(), \
        PRIMARY KEY (provider, family))",
    )
    .execute(&pool)
    .await
    .unwrap();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(catalog(&["v4p2-flash"])))
        .expect(1)
        .mount(&server)
        .await;
    successful_probe(&server, "v4p2-flash", 1).await;
    let first = resolver(&server);
    assert_eq!(
        first
            .resolve("synthetic-key", None, Some(&pool))
            .await
            .unwrap(),
        model("v4p2-flash")
    );
    let saved: String = sqlx::query_scalar("SELECT model FROM llm_model_cache")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(saved, model("v4p2-flash"));
    server.verify().await;
    server.reset().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&server)
        .await;
    successful_probe(&server, "v4p2-flash", 1).await;
    let restarted = resolver(&server);
    assert_eq!(
        restarted
            .resolve("synthetic-key", None, Some(&pool))
            .await
            .unwrap(),
        model("v4p2-flash")
    );
    sqlx::query("UPDATE llm_model_cache SET resolved_at = now() - interval '2 days'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(restarted.load_from_db(&pool).await.is_none());
    sqlx::query("UPDATE llm_model_cache SET resolved_at = now(), model = 'accounts/fireworks/models/deepseek-v99-pro'")
        .execute(&pool).await.unwrap();
    assert!(restarted.load_from_db(&pool).await.is_none());
    sqlx::query("UPDATE llm_model_cache SET model = $1, family = 'alte-policy'")
        .bind(model("v4p2-flash"))
        .execute(&pool)
        .await
        .unwrap();
    assert!(restarted.load_from_db(&pool).await.is_none());
    pool.close().await;
    sqlx::QueryBuilder::<sqlx::Postgres>::new("DROP SCHEMA ")
        .push(&schema)
        .push(" CASCADE")
        .build()
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}

#[tokio::test]
async fn echter_hub_pfad_heilt_404_genau_einmal_auf_neue_version() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(catalog(&["v4p1-flash"])))
        .expect(1)
        .mount(&server)
        .await;
    successful_probe(&server, "v4p1-flash", 1).await;
    let resolver = resolver(&server);
    resolver.resolve("synthetic-key", None, None).await.unwrap();
    server.verify().await;
    server.reset().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(catalog(&["v4p2-flash"])))
        .expect(1)
        .mount(&server)
        .await;
    successful_probe(&server, "v4p2-flash", 1).await;
    Mock::given(method("POST"))
        .and(body_partial_json(
            serde_json::json!({"model": model("v4p1-flash")}),
        ))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(body_partial_json(
            serde_json::json!({"model": model("v4p2-flash"),
            "messages": [{"role":"user", "content":"Synthetischer Kontrollfall"}]}),
        ))
        .respond_with(reply("Kontrolle erfolgreich"))
        .expect(1)
        .mount(&server)
        .await;
    let mut endpoint = LlmEndpoint {
        provider: "fireworks",
        model: model("v4p1-flash"),
        base_url: server.uri(),
        api_key: Some("synthetic-key".into()),
    };
    let response = crate::hub::call_with_resolver(
        &resolver,
        &mut endpoint,
        &Request::prompt("Synthetischer Kontrollfall").no_ledger(),
        None,
        Duration::from_secs(10),
        None,
    )
    .await
    .unwrap();
    assert_eq!(response.model, model("v4p2-flash"));
    assert_eq!(response.text, "Kontrolle erfolgreich");
}
