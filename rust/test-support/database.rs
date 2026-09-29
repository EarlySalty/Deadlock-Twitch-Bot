//! Shared PostgreSQL test configuration. Local runs use a normal JSON file;
//! existing CI variables remain a compatibility fallback.
#![allow(dead_code)]

fn local_config() -> Option<serde_json::Value> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find(|path| path.join("test-support").is_dir())
        .expect("Rust workspace root");
    match std::fs::read(root.join("test-database.json")) {
        Ok(bytes) => Some(serde_json::from_slice(&bytes).expect("valid test-database.json")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => panic!("cannot read local test-database.json"),
    }
}

pub fn database_url() -> Option<String> {
    if let Some(config) = local_config() {
        return Some(config["database_url"].as_str().expect("database_url string").to_owned());
    }
    std::env::var("TB_TEST_DATABASE_URL")
        .or_else(|_| std::env::var("TEST_DATABASE_URL"))
        .ok()
}

pub fn required() -> bool {
    local_config()
        .map(|config| config["require_database"].as_bool().unwrap_or(true))
        .unwrap_or_else(|| std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1"))
}
