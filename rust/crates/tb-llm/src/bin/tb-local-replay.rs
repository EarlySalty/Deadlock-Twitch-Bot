use std::path::Path;
use tb_llm::local_eval::{run, Config};

#[tokio::main]
async fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let (path, validate_only) = match args.as_slice() {
        [path] => (path, false),
        [path, flag] if flag == "--validate-only" => (path, true),
        _ => {
            eprintln!("Aufruf: tb-local-replay <private-config.json> [--validate-only]");
            std::process::exit(2);
        }
    };
    let outcome = match Config::load(Path::new(path)) {
        Ok(config) => run(config, validate_only).await,
        Err(error) => Err(error),
    };
    if let Err(error) = outcome {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
