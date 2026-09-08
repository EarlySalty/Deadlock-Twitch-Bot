use super::{
    dataset::SYSTEM_CARD, files, Dataset, EvalError, EvalResponse, LocalClient, Result, Variant,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub scope: String,
    pub base_url: String,
    pub model: String,
    pub dataset_path: PathBuf,
    pub dataset_sha256: String,
    pub output_dir: PathBuf,
    pub limit: usize,
    pub baseline_limit: usize,
    pub max_tokens: i64,
    pub timeout_secs: u64,
    pub temperature: f64,
    pub code_revision: String,
    #[serde(default)]
    pub compare_with: Option<PathBuf>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        serde_json::from_slice(&files::read_private(path, 65536)?)
            .map_err(|_| EvalError("config_json"))
    }
    fn validate(&self) -> Result<()> {
        if self.scope != "twitch"
            || self.limit == 0
            || self.limit > 50
            || self.baseline_limit > 10
            || self.baseline_limit > self.limit
            || !(1..=512).contains(&self.max_tokens)
            || !(1..=600).contains(&self.timeout_secs)
            || !self.temperature.is_finite()
            || !(0.0..=1.0).contains(&self.temperature)
            || self.dataset_sha256.len() != 64
            || !self.dataset_sha256.bytes().all(|c| c.is_ascii_hexdigit())
            || self.code_revision.len() != 40
            || !self.code_revision.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err(EvalError("config_limits"));
        }
        files::check_path(&self.output_dir)?;
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
struct Record {
    case_id: String,
    variant: Variant,
    model: String,
    prompt_bytes: usize,
    prompt_sha256: String,
    wall_time_ms: u64,
    error_code: Option<String>,
    response: Option<EvalResponse>,
}

struct PriorRun {
    records: Vec<Record>,
    manifest: serde_json::Value,
}

fn load_prior(
    config: &Config,
    data: &Dataset,
    jobs: &[(usize, Variant, String)],
) -> Result<Option<PriorRun>> {
    let Some(directory) = &config.compare_with else {
        return Ok(None);
    };
    let manifest_bytes = files::read_private(&directory.join("manifest.json"), 65536)?;
    let manifest: serde_json::Value =
        serde_json::from_slice(&manifest_bytes).map_err(|_| EvalError("comparison_manifest"))?;
    let previous: Config = serde_json::from_value(manifest["config"].clone())
        .map_err(|_| EvalError("comparison_config"))?;
    previous.validate()?;
    LocalClient::new(&previous.scope, &previous.base_url, &previous.model)?;
    let binary = std::fs::read(std::env::current_exe().map_err(|_| EvalError("binary_path"))?)
        .map_err(|_| EvalError("binary_read"))?;
    if manifest["binary_sha256"] != files::sha256(&binary) {
        return Err(EvalError("comparison_binary"));
    }
    if previous.scope != config.scope
        || previous.model == config.model
        || !matches!(
            previous.model.as_str(),
            "qwen3.5-4b-local" | "qwen3.5-9b-local"
        )
        || previous.dataset_sha256 != config.dataset_sha256
        || previous.limit != config.limit
        || previous.baseline_limit != config.baseline_limit
        || previous.max_tokens != config.max_tokens
        || previous.temperature != config.temperature
        || previous.timeout_secs != config.timeout_secs
        || previous.code_revision != config.code_revision
        || manifest["system_card_sha256"] != files::sha256(SYSTEM_CARD.as_bytes())
    {
        return Err(EvalError("comparison_parameters"));
    }
    let summary_bytes = files::read_private(&directory.join("summary.json"), 65536)?;
    let summary: serde_json::Value =
        serde_json::from_slice(&summary_bytes).map_err(|_| EvalError("comparison_summary"))?;
    if summary["complete"] != true {
        return Err(EvalError("comparison_incomplete"));
    }
    let bytes = files::read_private(&directory.join("responses.jsonl"), 4 * 1024 * 1024)?;
    let contents = std::str::from_utf8(&bytes).map_err(|_| EvalError("comparison_jsonl"))?;
    let records: Vec<Record> = contents
        .lines()
        .map(|line| serde_json::from_str(line).map_err(|_| EvalError("comparison_jsonl")))
        .collect::<Result<_>>()?;
    if records.len() != jobs.len() {
        return Err(EvalError("comparison_count"));
    }
    let mut seen = std::collections::HashSet::new();
    for record in &records {
        let (_, _, prompt) = jobs
            .iter()
            .find(|(index, variant, _)| {
                data.cases[*index].case_id == record.case_id && *variant == record.variant
            })
            .ok_or(EvalError("comparison_case"))?;
        if !seen.insert((&record.case_id, record.variant))
            || record.model != previous.model
            || record.prompt_sha256 != files::sha256(format!("{SYSTEM_CARD}\n{prompt}").as_bytes())
            || record
                .response
                .as_ref()
                .is_some_and(|r| r.model != previous.model || r.text.len() > 65536)
        {
            return Err(EvalError("comparison_record"));
        }
    }
    Ok(Some(PriorRun {
        records,
        manifest: json!({"directory":directory,"manifest_sha256":files::sha256(&manifest_bytes),"responses_sha256":files::sha256(&bytes),"model":previous.model}),
    }))
}

pub async fn run(config: Config, validate_only: bool) -> Result<()> {
    config.validate()?;
    let client = LocalClient::new(&config.scope, &config.base_url, &config.model)?;
    let bytes = files::read_private(&config.dataset_path, 4 * 1024 * 1024)?;
    if files::sha256(&bytes) != config.dataset_sha256 {
        return Err(EvalError("dataset_sha_mismatch"));
    }
    let data: Dataset = serde_json::from_slice(&bytes).map_err(|_| EvalError("dataset_json"))?;
    data.validate()?;
    if data.cases.len() < config.limit {
        return Err(EvalError("case_count"));
    }
    let mut jobs = Vec::new();
    for (index, case) in data.cases.iter().take(config.limit).enumerate() {
        jobs.push((
            index,
            Variant::Contextual,
            case.prompt(Variant::Contextual)?,
        ));
        if index < config.baseline_limit {
            jobs.push((index, Variant::Baseline, case.prompt(Variant::Baseline)?));
        }
    }
    let prior = load_prior(&config, &data, &jobs)?;
    if validate_only {
        println!(
            "Validiert: {} Fälle, {} Modellaufrufe vorbereitet, keine ausgeführt.",
            config.limit,
            jobs.len()
        );
        return Ok(());
    }
    files::create_directory(&config.output_dir)?;
    let executable = std::env::current_exe().map_err(|_| EvalError("binary_path"))?;
    let binary_bytes = std::fs::read(executable).map_err(|_| EvalError("binary_read"))?;
    files::write(&config.output_dir.join("manifest.json"), &serde_json::to_vec_pretty(&json!({
        "config":config, "started_at":chrono::Utc::now().to_rfc3339(),
        "binary_sha256":files::sha256(&binary_bytes), "system_card_sha256":files::sha256(SYSTEM_CARD.as_bytes()),
        "model_calls_planned":jobs.len(),"dataset_sha256_verified":true,
        "comparison_source":prior.as_ref().map(|p|&p.manifest),
        "scope":"twitch", "cloud_calls":0, "sent_messages":0, "human_quality_review":"ausstehend",
        "provenance_limitations":["Account-Zuordnung beweist keine manuelle Autorschaft.","Stream-Audio hat unbekannte Sprecher; damalige STT-Verfügbarkeit ist nicht vollständig belegt.","Historische Referenzen beweisen keine Wirkung der generierten Antworten."]
    })).map_err(|_| EvalError("manifest_json"))?)?;
    let mut raw = files::create_file(&config.output_dir.join("responses.jsonl"))?;
    let mut records = Vec::new();
    let run_start = Instant::now();
    checkpoint_summary(&config, &data, &records, 0, false)?;
    for (index, variant, prompt) in jobs {
        let prompt_bytes = SYSTEM_CARD.len() + prompt.len();
        let prompt_sha256 = files::sha256(format!("{SYSTEM_CARD}\n{prompt}").as_bytes());
        let started = Instant::now();
        let result = client
            .complete(
                crate::Request::simple(SYSTEM_CARD, prompt)
                    .max_tokens(config.max_tokens)
                    .temperature(config.temperature)
                    .timeout_secs(config.timeout_secs)
                    .denken_aus()
                    .no_ledger(),
            )
            .await;
        let record = Record {
            case_id: data.cases[index].case_id.clone(),
            variant,
            model: config.model.clone(),
            prompt_bytes,
            prompt_sha256,
            wall_time_ms: started.elapsed().as_millis() as u64,
            error_code: match &result {
                Err(e) => Some(e.0.into()),
                Ok(response) => response.error_code.clone(),
            },
            response: result.ok(),
        };
        serde_json::to_writer(&mut raw, &record).map_err(|_| EvalError("response_json"))?;
        raw.write_all(b"\n")
            .and_then(|()| raw.flush())
            .map_err(|_| EvalError("response_write"))?;
        records.push(record);
        checkpoint_summary(
            &config,
            &data,
            &records,
            run_start.elapsed().as_millis() as u64,
            false,
        )?;
        println!(
            "Fortschritt: {}/{} Aufrufe, {} Fehler.",
            records.len(),
            config.limit + config.baseline_limit,
            records.iter().filter(|r| r.error_code.is_some()).count()
        );
    }
    raw.sync_all().map_err(|_| EvalError("response_write"))?;
    let elapsed = run_start.elapsed().as_millis() as u64;
    let report = format!("# Privater Twitch-Replay\n\nModell: {}. Vollständig: {} Fälle mit Kontext und {} identische Baselines.\n\nLaufzeit: {:.1} Minuten. Fehler: {}. Die Baseline ist nur gegen dieselben {} verbesserten Fälle vergleichbar.\n\nAlle Antworten stehen in comparison.html. Die sprachliche Qualität wurde noch nicht menschlich bewertet. Historische Antworten sind Referenzen, keine einzig richtige Lösung. Keine Nachrichten versendet, kein Cloudaufruf, kein Training, kein Concierge-Stil und kein Produktionsmodellwechsel.\n\nTechnische Aggregate stehen in summary.json, reproduzierbare Parameter und Prüfsummen in manifest.json.\n", config.model, config.limit, config.baseline_limit, elapsed as f64 / 60000.0, records.iter().filter(|r|r.error_code.is_some()).count(), config.baseline_limit);
    files::write(&config.output_dir.join("REPORT.md"), report.as_bytes())?;
    let current_records_len = records.len();
    if let Some(prior) = prior {
        records.extend(prior.records);
    }
    files::write(
        &config.output_dir.join("comparison.html"),
        render(&data, &records, &config).as_bytes(),
    )?;
    checkpoint_summary(
        &config,
        &data,
        &records[..current_records_len],
        elapsed,
        true,
    )?;
    println!(
        "Abgeschlossen: {} Antworten/Fehler protokolliert; private Berichte erstellt.",
        config.limit + config.baseline_limit
    );
    Ok(())
}

fn checkpoint_summary(
    config: &Config,
    data: &Dataset,
    records: &[Record],
    elapsed: u64,
    complete: bool,
) -> Result<()> {
    let summary = json!({"model":config.model,"complete":complete && records.len()==config.limit+config.baseline_limit,
        "planned":config.limit+config.baseline_limit,"completed":records.len(),"status":if complete {"complete"}else{"incomplete"},
        "wall_time_ms":elapsed,"all":summarize(&records.iter().collect::<Vec<_>>()),
        "contextual_all":summarize(&records.iter().filter(|r|r.variant==Variant::Contextual).collect::<Vec<_>>()),
        "baseline_matched":summarize(&records.iter().filter(|r|r.variant==Variant::Baseline).collect::<Vec<_>>()),
        "contextual_matched":summarize(&records.iter().filter(|r|r.variant==Variant::Contextual && data.cases.iter().take(config.baseline_limit).any(|c|c.case_id==r.case_id)).collect::<Vec<_>>()),
        "quality_verdict":"Noch nicht menschlich bewertet; keine Aussage über Konversion oder autonome Einsatzreife."});
    files::checkpoint(
        &config.output_dir.join("summary.json"),
        &serde_json::to_vec_pretty(&summary).map_err(|_| EvalError("summary_json"))?,
    )?;
    Ok(())
}

fn summarize(records: &[&Record]) -> serde_json::Value {
    let mut times: Vec<_> = records.iter().map(|r| r.wall_time_ms).collect();
    times.sort_unstable();
    let quantile = |p: f64| {
        if times.is_empty() {
            None
        } else {
            Some(times[((times.len() as f64 * p).ceil() as usize).saturating_sub(1)])
        }
    };
    let tokens: i64 = records
        .iter()
        .filter_map(|r| r.response.as_ref()?.completion_tokens)
        .sum();
    let duration: u64 = times.iter().sum();
    json!({"attempts":records.len(),"responses":records.iter().filter(|r|r.error_code.is_none()).count(),
        "errors":records.iter().filter(|r|r.error_code.is_some()).count(),
        "empty":records.iter().filter(|r|r.error_code.as_deref()==Some("empty_answer")).count(),
        "truncated":records.iter().filter(|r|r.response.as_ref().is_some_and(|s|s.finish_reason=="length")).count(),
        "wall_latency_median_ms":quantile(0.5),"wall_latency_p95_ms":quantile(0.95),
        "completion_tokens_recorded":tokens,"completion_tokens_per_total_request_second":if duration>0 {Some(tokens as f64*1000.0/duration as f64)} else {None},
        "prompt_bytes_max":records.iter().map(|r|r.prompt_bytes).max(),
        "human_language_verdict":"ausstehend"})
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn render(data: &Dataset, records: &[Record], config: &Config) -> String {
    let mut html = String::from("<!doctype html><html lang=\"de\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'\"><title>Privater Twitch-Vergleich</title><style>body{font:17px system-ui;margin:2rem auto;padding:0 1rem;max-width:1100px;background:#121212;color:#eee}article{border:1px solid #c8a86b;padding:1rem;margin:1.5rem 0}pre{white-space:pre-wrap;overflow-wrap:anywhere;font:inherit}section{display:grid;grid-template-columns:repeat(auto-fit,minmax(230px,1fr));gap:1rem}.answer{background:#222;padding:1rem}summary{cursor:pointer}h2{font-size:1.1rem}</style><h1>Privater Twitch-Vergleich</h1><p>Noch nicht menschlich bewertet. Historische Referenzen sind keine einzig richtige Lösung. Kein Text wurde versendet.</p>");
    html.push_str(&format!(
        "<p>Modell: {}. Mit Kontext: {} Fälle; Baseline: dieselben ersten {} Fälle.</p>",
        escape(&config.model),
        config.limit,
        config.baseline_limit
    ));
    for case in data.cases.iter().take(config.limit) {
        html.push_str(&format!("<article><h2>{}</h2><details><summary>Vorheriger Kontext</summary><pre>{}</pre></details><section><div class=\"answer\"><strong>Echte historische Antwort</strong><pre>{}</pre></div>",escape(&case.case_id),escape(&case.context.iter().map(|c|format!("{} [{}]: {}",c.kind,c.author_id,c.text)).collect::<Vec<_>>().join("\n")),escape(&case.reference.text)));
        for record in records.iter().filter(|r| r.case_id == case.case_id) {
            let title = if record.variant == Variant::Contextual {
                "Mit Stil und Wissen"
            } else {
                "Baseline ohne Stil und Zusatzwissen"
            };
            let text = record
                .response
                .as_ref()
                .filter(|_| record.error_code.is_none())
                .map(|s| s.text.as_str())
                .unwrap_or("Keine verwertbare Antwort");
            let status = record.error_code.as_deref().unwrap_or_else(|| {
                record
                    .response
                    .as_ref()
                    .map(|s| s.finish_reason.as_str())
                    .unwrap_or("missing")
            });
            html.push_str(&format!("<div class=\"answer\"><strong>{} · {title}</strong><pre>{}</pre><p>{:.1} s · {}</p></div>",escape(&record.model),escape(text),record.wall_time_ms as f64/1000.0,escape(status)));
        }
        html.push_str("</section></article>");
    }
    html.push_str("</html>");
    html
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn html_data_cannot_create_markup() {
        assert_eq!(
            escape("<script>\"x\" & 'y'</script>"),
            "&lt;script&gt;&quot;x&quot; &amp; &#39;y&#39;&lt;/script&gt;"
        );
    }
}
