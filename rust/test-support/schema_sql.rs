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
