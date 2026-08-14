//! Backtest des Smalltalks gegen echte eigene Chatzeilen.
//!
//! Der Live-Schattenlauf (`smalltalk_loop_wiring`) braucht einen sendenden
//! Streamer, eine Stunde Zeit und liefert am Ende einen Discord-Report, den
//! niemand mit einem zweiten Prompt vergleichen kann. Dieses Werkzeug macht
//! dasselbe offline, in Minuten, wiederholbar und gegen einen Massstab: die
//! Zeile, die der Owner in genau dieser Lage wirklich getippt hat.
//!
//! ```text
//! TWITCH_ANALYTICS_DSN=… tb-smalltalk-bench --limit 40
//! TWITCH_ANALYTICS_DSN=… tb-smalltalk-bench --variant test_mode --limit 40
//! ```
//!
//! Zwei Laeufe ueber dieselben Samples sind direkt vergleichbar: die Auswahl
//! haengt an `--limit` und `--channel`, nicht am Zufall, und auch die
//! Seitenverteilung der Blindprobe steht fest (siehe
//! [`tb_engagement::smalltalk_bench::ai_first_for`]).
//!
//! Modell und Anbieter kommen aus der zentralen Auswahl des Bots
//! (`tb_llm::endpoint_for("engagement")`). Das Werkzeug bringt keinen eigenen
//! Connector mit, sonst misst der Backtest ein Modell, das im Livebetrieb gar
//! nicht antwortet.

use std::time::Duration;

use sqlx::postgres::PgPoolOptions;
use tb_engagement::llm_chat::{
    build_test_mode_system_prompt, sanitize_test_mode_text, EngagementLlmClient,
};
use tb_engagement::smalltalk_bench::{
    judge, load_samples, BenchLine, BenchStore, JudgePick, JudgeVerdict,
};
use tb_engagement::smalltalk_v1::{self, style_profile, Outcome, Stimulus, StyleProfile};

/// Zeitlimit je Modell-Call. Grosszuegiger als im Chat: hier wartet niemand
/// live, und ein Timeout kostet ein Sample aus der Messung.
const CALL_TIMEOUT: Duration = Duration::from_secs(60);
/// Grund, unter dem eine leere Modellantwort abgelegt wird. Kein Wert aus
/// `TestModeRejectReason`: der Ausgabefilter hat hier gar nichts verworfen,
/// es kam schlicht nichts an.
const EMPTY_REASON: &str = "leere_antwort";
/// Token-Budget der Vergleichsvariante. Gleicher Grund wie in
/// [`tb_engagement::smalltalk_v1`]: das Modell denkt aus demselben Kontingent.
const TEST_MODE_MAX_TOKENS: i64 = 600;
/// Wie viele Zeilen am Ende nebeneinander ausgedruckt werden.
const PREVIEW_LINES: usize = 20;

struct Args {
    limit: i64,
    channel: Option<String>,
    variant: String,
    judge: bool,
    note: Option<String>,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            limit: 40,
            channel: None,
            variant: "v1".to_string(),
            judge: true,
            note: None,
        }
    }
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args::default();
    let mut raw = std::env::args().skip(1);
    while let Some(flag) = raw.next() {
        match flag.as_str() {
            "--limit" => {
                args.limit = raw
                    .next()
                    .ok_or("--limit braucht eine Zahl")?
                    .parse()
                    .map_err(|_| "--limit braucht eine Zahl".to_string())?;
            }
            "--channel" => args.channel = Some(raw.next().ok_or("--channel braucht einen Namen")?),
            "--variant" => {
                let value = raw.next().ok_or("--variant braucht v1 oder test_mode")?;
                if value != "v1" && value != "test_mode" {
                    return Err(format!(
                        "unbekannte Variante {value}, erlaubt: v1, test_mode"
                    ));
                }
                args.variant = value;
            }
            "--no-judge" => args.judge = false,
            "--note" => args.note = Some(raw.next().ok_or("--note braucht einen Text")?),
            "--help" | "-h" => {
                println!(
                    "tb-smalltalk-bench [--limit N] [--channel login] \
[--variant v1|test_mode] [--no-judge] [--note text]"
                );
                std::process::exit(0);
            }
            other => return Err(format!("unbekanntes Argument {other}")),
        }
    }
    Ok(args)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let args = parse_args().map_err(|err| -> Box<dyn std::error::Error> { err.into() })?;
    let dsn = std::env::var("TWITCH_ANALYTICS_DSN")
        .map_err(|_| "TWITCH_ANALYTICS_DSN fehlt".to_string())?;
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&dsn)
        .await?;

    let samples = load_samples(&pool, args.limit, args.channel.as_deref(), true).await?;
    if samples.is_empty() {
        eprintln!("keine Samples gefunden");
        return Ok(());
    }

    let client = EngagementLlmClient::new(None, None, None, Some(CALL_TIMEOUT));
    let endpoint = tb_llm::endpoint_for("engagement");
    let store = BenchStore::new(pool.clone());
    let judge_model = args.judge.then(|| client.model().to_string());
    let run_id = store
        .start_run(
            &args.variant,
            client.model(),
            judge_model.as_deref(),
            args.note.as_deref(),
        )
        .await?;

    println!(
        "Lauf {run_id}\nVariante {}\nAnbieter {} / {}\nSamples {}\n",
        args.variant,
        endpoint.provider,
        client.model(),
        samples.len()
    );

    let mut lines = Vec::with_capacity(samples.len());
    for (index, sample) in samples.iter().enumerate() {
        eprint!("\r{}/{}", index + 1, samples.len());
        let stimulus = Stimulus {
            channel_login: sample.channel_login.clone(),
            stream_context: sample.stream_context.clone(),
            chat_context: sample.chat_context.clone(),
        };

        let generated = match generate(&client, &args.variant, &stimulus).await {
            Ok(generated) => generated,
            Err(err) => {
                eprintln!("\nSample {}: {err}", sample.id);
                continue;
            }
        };

        // Der Ausgabefilter des Livebetriebs entscheidet auch hier. Eine Zeile,
        // die nie gesendet wuerde, darf die Stilmessung nicht schoenen.
        // Schweigen und leere Antwort landen beide ohne Text in der Ablage,
        // bleiben ueber `reject_reason` aber unterscheidbar.
        let (ai_text, reject_reason) = match &generated.outcome {
            Outcome::Silent => (None, None),
            Outcome::Empty => (None, Some(EMPTY_REASON.to_string())),
            Outcome::Line(text) => match sanitize_test_mode_text(text) {
                Ok(clean) => (Some(clean), None),
                Err(reason) => (None, Some(reason.as_str().to_string())),
            },
        };

        let verdict = match (args.judge, ai_text.as_deref()) {
            (true, Some(text)) => match judge(&client, sample.id, &sample.human_text, text).await {
                Ok(verdict) => Some(verdict),
                Err(err) => {
                    eprintln!("\nBlindprobe {} fehlgeschlagen: {err}", sample.id);
                    None
                }
            },
            _ => None,
        };

        let line = BenchLine {
            sample_id: sample.id,
            channel_login: sample.channel_login.clone(),
            message_ts: sample.message_ts,
            human_text: sample.human_text.clone(),
            stream_context: sample.stream_context.clone(),
            chat_context: sample.chat_context.clone(),
            ai_text,
            reject_reason,
            latency_ms: i32::try_from(generated.latency_ms).ok(),
            judge: verdict,
        };
        store.record_line(run_id, &line).await?;
        lines.push(line);
    }
    eprintln!();

    let count = i32::try_from(lines.len()).unwrap_or(i32::MAX);
    store.finish_run(run_id, count).await?;
    report(&lines);
    Ok(())
}

/// Erzeugt die KI-Zeile in der gewaehlten Variante.
///
/// `test_mode` nutzt den grossen Prompt des Live-Schattenlaufs, damit sich der
/// schlanke V1-Prompt gegen den Bestand messen laesst und nicht gegen nichts.
async fn generate(
    client: &EngagementLlmClient,
    variant: &str,
    stimulus: &Stimulus,
) -> Result<smalltalk_v1::Generated, tb_engagement::llm_chat::GenerateError> {
    if variant == "v1" {
        return smalltalk_v1::generate(client, stimulus).await;
    }
    let system = build_test_mode_system_prompt(&stimulus.channel_login);
    let user = smalltalk_v1::build_user_prompt(stimulus);
    let started = std::time::Instant::now();
    let raw = client
        .raw_completion_tracked(&system, &user, TEST_MODE_MAX_TOKENS, 0.7, "smalltalk-bench")
        .await?;
    Ok(smalltalk_v1::Generated {
        outcome: smalltalk_v1::classify(&raw),
        model: client.model().to_string(),
        latency_ms: i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX),
    })
}

fn report(lines: &[BenchLine]) {
    let human: Vec<String> = lines.iter().map(|l| l.human_text.clone()).collect();
    let ai: Vec<String> = lines.iter().filter_map(|l| l.ai_text.clone()).collect();
    let human_profile = style_profile(&human);
    let ai_profile = style_profile(&ai);

    println!("--- Stil: Mensch gegen KI ---");
    print_profiles(&human_profile, &ai_profile);

    let silent = lines
        .iter()
        .filter(|l| l.ai_text.is_none() && l.reject_reason.is_none())
        .count();
    let empty = lines
        .iter()
        .filter(|l| l.reject_reason.as_deref() == Some(EMPTY_REASON))
        .count();
    let rejected = lines
        .iter()
        .filter(|l| l.reject_reason.is_some() && l.reject_reason.as_deref() != Some(EMPTY_REASON))
        .count();
    let total = lines.len();
    println!(
        "\nvon {total}: {} gesendet ({:.0}%), {silent} geschwiegen ({:.0}%), \
{rejected} vom Filter verworfen ({:.0}%), {empty} leer geblieben ({:.0}%)",
        ai.len(),
        share(ai.len(), total) * 100.0,
        share(silent, total) * 100.0,
        share(rejected, total) * 100.0,
        share(empty, total) * 100.0
    );
    print_reject_reasons(lines);

    // Der Massstab gilt fuer beide Seiten: wie viele der echten Zeilen wuerde
    // der Ausgabefilter selbst wegwerfen. Liegt die Quote hoch, ist nicht die
    // KI zu auffaellig, sondern der Filter zu streng.
    let human_rejected = human
        .iter()
        .filter(|text| sanitize_test_mode_text(text).is_err())
        .count();
    println!(
        "Zum Vergleich: der Ausgabefilter wuerde {human_rejected} der {} echten Zeilen \
verwerfen ({:.0}%)",
        human.len(),
        share(human_rejected, human.len()) * 100.0
    );

    print_judge(lines);
    print_preview(lines);
}

fn print_profiles(human: &StyleProfile, ai: &StyleProfile) {
    let row = |label: &str, a: String, b: String| println!("{label:<24} {a:>10} {b:>10}");
    row(
        "",
        format!("Mensch ({})", human.n),
        format!("KI ({})", ai.n),
    );
    row(
        "Zeichen im Schnitt",
        format!("{:.1}", human.avg_chars),
        format!("{:.1}", ai.avg_chars),
    );
    row(
        "Zeichen Median",
        human.median_chars.to_string(),
        ai.median_chars.to_string(),
    );
    row(
        "Woerter im Schnitt",
        format!("{:.1}", human.avg_words),
        format!("{:.1}", ai.avg_words),
    );
    let percent = |value: f64| format!("{:.0}%", value * 100.0);
    row(
        "bis 15 Zeichen",
        percent(human.very_short_share),
        percent(ai.very_short_share),
    );
    row(
        "klein angefangen",
        percent(human.lower_start_share),
        percent(ai.lower_start_share),
    );
    row(
        "mit Fragezeichen",
        percent(human.question_share),
        percent(ai.question_share),
    );
    row(
        "mit Komma",
        percent(human.comma_share),
        percent(ai.comma_share),
    );
    row(
        "zweiter Satz",
        percent(human.two_sentence_share),
        percent(ai.two_sentence_share),
    );
    row(
        "Lacher drin",
        percent(human.laughter_share),
        percent(ai.laughter_share),
    );
    row(
        "Punkt am Ende",
        percent(human.trailing_period_share),
        percent(ai.trailing_period_share),
    );
    row(
        "verschiedene Opener",
        percent(human.distinct_opener_share),
        percent(ai.distinct_opener_share),
    );
}

fn print_reject_reasons(lines: &[BenchLine]) {
    let mut reasons: Vec<(&str, usize)> = Vec::new();
    for line in lines {
        let Some(reason) = line.reject_reason.as_deref() else {
            continue;
        };
        match reasons.iter_mut().find(|(name, _)| *name == reason) {
            Some((_, count)) => *count += 1,
            None => reasons.push((reason, 1)),
        }
    }
    reasons.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    for (reason, count) in reasons {
        println!("  {reason}: {count}");
    }
}

fn print_judge(lines: &[BenchLine]) {
    let verdicts: Vec<JudgeVerdict> = lines.iter().filter_map(|line| line.judge).collect();
    if verdicts.is_empty() {
        return;
    }
    let decided = verdicts
        .iter()
        .filter(|verdict| verdict.pick != JudgePick::Unsure)
        .count();
    let correct = verdicts
        .iter()
        .filter(|verdict| verdict.correct == Some(true))
        .count();
    println!(
        "\n--- Blindprobe ---\n{correct} von {decided} mal die KI erkannt ({:.0}%). \
50% heisst ununterscheidbar, ab 80% faellt sie auf.",
        share(correct, decided) * 100.0
    );
    let unsure = verdicts.len() - decided;
    if unsure > 0 {
        println!("{unsure} Urteile waren nicht auswertbar");
    }
}

fn print_preview(lines: &[BenchLine]) {
    println!("\n--- Zeilen nebeneinander ---");
    for line in lines.iter().take(PREVIEW_LINES) {
        let ai = line
            .ai_text
            .clone()
            .unwrap_or_else(|| match line.reject_reason.as_deref() {
                Some(reason) => format!("[verworfen: {reason}]"),
                None => "[still]".to_string(),
            });
        println!("{:<10} Mensch: {}", line.channel_login, line.human_text);
        println!("{:<10} KI    : {ai}", "");
    }
}

fn share(part: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        part as f64 / total as f64
    }
}
