use std::path::Path;
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_ansi(false).init();
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 || args[1] != "--config" {
        eprintln!("Aufruf: tb-model-resolver --config /absoluter/pfad.toml");
        std::process::exit(2);
    }
    let result = match tb_llm::daily_model_resolver::Config::load(Path::new(&args[2])) {
        Ok(config) => tb_llm::daily_model_resolver::run(config).await,
        Err(error) => Err(error),
    };
    if let Err(error) = result {
        eprintln!("Resolverprüfung fehlgeschlagen: {error}");
        std::process::exit(1);
    }
}
