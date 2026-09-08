use std::fs;
use std::path::{Path, PathBuf};

const NEEDLE: &str = "minimax";
const ALLOWED_TOKEN: &str = "minimax_reasoning";

const ALLOWED_FILES: &[&str] = &[
    "rust/crates/tb-llm/src/hub.rs",
    "rust/crates/tb-llm/src/selection.rs",
    "rust/crates/tb-llm/tests/no_minimax_identifiers.rs",
];

const SKIP_DIRS: &[&str] = &["target", ".sqlx", "migrations", "docs", "node_modules", "dist"];

const SCAN_EXTS: &[&str] = &["rs", "ts", "tsx", "js", "jsx", "sql", "toml", "txt"];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("Repo-Wurzel oberhalb von rust/crates/tb-llm")
        .to_path_buf()
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn is_allowed_file(rel_path: &str) -> bool {
    ALLOWED_FILES.contains(&rel_path)
}

fn line_hat_minimax(line: &str) -> bool {
    line.to_lowercase()
        .replace(ALLOWED_TOKEN, "")
        .contains(NEEDLE)
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if SKIP_DIRS.contains(&name) {
                continue;
            }
            collect_files(&path, out);
        } else {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default();
            if SCAN_EXTS.contains(&ext) {
                out.push(path);
            }
        }
    }
}

#[test]
fn kein_minimax_bezeichner_mehr_im_code() {
    let root = repo_root();
    let mut files = Vec::new();
    collect_files(&root.join("rust"), &mut files);
    collect_files(&root.join("bot/dashboard_v2/src"), &mut files);
    files.sort();

    let mut hits: Vec<String> = Vec::new();
    for path in &files {
        let rel_path = rel(&root, path);
        if is_allowed_file(&rel_path) {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_lowercase();
        if name.contains(NEEDLE) {
            hits.push(format!("{rel_path}:0: Dateiname enthaelt \"{NEEDLE}\""));
        }
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for (i, line) in content.lines().enumerate() {
            if line_hat_minimax(line) {
                hits.push(format!("{rel_path}:{}: {}", i + 1, line.trim()));
            }
        }
    }

    if !hits.is_empty() {
        let shown: Vec<String> = hits.iter().take(40).cloned().collect();
        panic!(
            "{} verbliebene minimax-Treffer (erste {}):\n{}",
            hits.len(),
            shown.len(),
            shown.join("\n")
        );
    }
}
