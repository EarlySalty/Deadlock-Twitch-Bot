//! Ein-Prozess-Handler für die socket-aktivierte Medienmaterialisierung.
//!
//! systemd übergibt bei `Accept=yes` genau eine verbundene Unix-Verbindung als
//! stdin/stdout. Der Prozess erhält weder Dienst-Secrets noch Datenbankzugriff.

#[tokio::main]
async fn main() {
    let valid_cli = std::env::args_os().count() == 2
        && std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--stdio"));
    if !valid_cli {
        std::process::exit(64);
    }
    if tb_social_media::downloader_ipc::serve_stdio()
        .await
        .is_err()
    {
        std::process::exit(1);
    }
}
