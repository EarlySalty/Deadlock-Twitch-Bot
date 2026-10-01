//! SQLx 0.9 schema statements for isolated database tests.
//! Only validated test identifiers can enter these non-bindable SQL positions.

fn checked(operation: &str, schema: &str, suffix: &str) -> sqlx::AssertSqlSafe<String> {
    assert!(!schema.is_empty());
    assert!(schema
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'));
    sqlx::AssertSqlSafe(format!("{operation} \"{schema}\"{suffix}"))
}

pub fn create_schema(schema: &str, if_absent: bool) -> sqlx::AssertSqlSafe<String> {
    checked(
        if if_absent {
            "CREATE SCHEMA IF NOT EXISTS"
        } else {
            "CREATE SCHEMA"
        },
        schema,
        "",
    )
}

pub fn drop_schema(schema: &str, if_present: bool) -> sqlx::AssertSqlSafe<String> {
    checked(
        if if_present {
            "DROP SCHEMA IF EXISTS"
        } else {
            "DROP SCHEMA"
        },
        schema,
        " CASCADE",
    )
}

pub fn search_path(schema: &str) -> sqlx::AssertSqlSafe<String> {
    checked("SET search_path TO", schema, "")
}

#[test]
fn schema_identifiers_reject_sql_fragments() {
    for invalid in [
        "",
        "quoted\"",
        "x;DROP TABLE users",
        "x y",
        "x.y",
        "x--",
        "ä",
    ] {
        assert!(std::panic::catch_unwind(|| create_schema(invalid, false)).is_err());
        assert!(std::panic::catch_unwind(|| drop_schema(invalid, true)).is_err());
        assert!(std::panic::catch_unwind(|| search_path(invalid)).is_err());
    }
}

#[test]
fn schema_identifiers_accept_isolated_test_names() {
    assert_eq!(
        create_schema("test_123", false).0,
        "CREATE SCHEMA \"test_123\""
    );
    assert_eq!(
        drop_schema("test_123", true).0,
        "DROP SCHEMA IF EXISTS \"test_123\" CASCADE"
    );
    assert_eq!(search_path("test_123").0, "SET search_path TO \"test_123\"");
}
