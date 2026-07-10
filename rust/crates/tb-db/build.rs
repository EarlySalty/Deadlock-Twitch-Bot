//! Sagt Cargo, dass `tb-db` von den Migrations-Dateien abhängt.
//!
//! `sqlx::migrate!` (siehe `src/migrate.rs`) bettet den Inhalt von
//! `rust/migrations/` zur Compile-Zeit in die Binary ein. Cargo sieht diese
//! Abhängigkeit von sich aus nicht: eine neue `.sql`-Datei ändert keine `.rs`
//! Datei, also gilt die Crate als unverändert und die Binary trägt weiter die
//! alte Migrationsliste. Bisher half nur ein `touch` von Hand, und das vergisst
//! man genau dann, wenn es darauf ankommt.
fn main() {
    println!("cargo:rerun-if-changed=../../migrations");
}
