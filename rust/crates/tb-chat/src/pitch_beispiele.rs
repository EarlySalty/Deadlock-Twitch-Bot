use sqlx::PgPool;

const MIN_LEARNED_GUT: usize = 4;
const MAX_GUT: i64 = 8;
const MAX_SCHLECHT: i64 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PitchPfad {
    Anlass,
    Partner,
    Periodic,
}

impl PitchPfad {
    pub fn log_pfad(self) -> &'static str {
        match self {
            Self::Anlass => "anlass",
            Self::Partner => "partner",
            Self::Periodic => "periodic",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Beispiel {
    pub ausloeser: Option<String>,
    pub antwort: String,
}

impl Beispiel {
    fn aus(ausloeser: &str, antwort: &str) -> Self {
        Self {
            ausloeser: Some(ausloeser.to_string()),
            antwort: antwort.to_string(),
        }
    }
}

const ANLASS_START: &[(&str, &str)] = &[
    (
        "hab keine leute zum zocken",
        "@{login} solo in deadlock ist wie solo in der lane, geht, macht aber keinen spaß. im discord der community hängen abends welche rum, die genau dich suchen :)",
    ),
    (
        "spiel ist tot",
        "@{login} totgesagte spiele leben länger, sagt der bot, der davon lebt. die deutsche community ist jedenfalls quicklebendig.",
    ),
    (
        "warum sind alle so tryhard",
        "@{login} weil deadlock leute anzieht, die drei stunden patchnotes lesen. gibt aber auch die entspannte fraktion, die trifft sich im community discord.",
    ),
    (
        "solo queue ist die hölle",
        "@{login} solo queue ist nicht die hölle, die hölle hat wenigstens feste teams. die community kennt das leid, da findet man leute für stack.",
    ),
    (
        "hab gestern angefangen, versteh nix",
        "@{login} willkommen, in den ersten tagen versteht niemand was, das gehört zum spiel. wenn du fragen hast, im community discord beißt keiner :)",
    ),
    (
        "welchen hero soll ich lernen",
        "@{login} den, der dir am wenigsten aufs maul gibt. ehrliche antwort gibts von den leuten im community discord, die spielen das länger als der bot existiert.",
    ),
];

const PARTNER_START: &[(&str, &str)] = &[
    (
        "nice, graves läuft bei dir",
        "@{login} graves läuft, der chat läuft, nur der bot hier läuft im kreis. wenn du öfter deadlock streamst: die deutsche deadlock community hat ein partner-netzwerk, wer offline geht, schickt seine zuschauer zum nächsten deutschen streamer und bekommt selbst raids zurück, dazu chat-schutz gegen spam und scam.",
    ),
    (
        "bin auch am streamen, kommt keiner",
        "@{login} kennt jeder, der mit drei zuschauern startet, zwei davon der bot. bei der deutschen deadlock community gibt es ein partner-netzwerk: raids gehen automatisch an deutsche deadlock-streamer, die gerade live sind, und kommen genauso zurück.",
    ),
    (
        "endlich mal jemand der auch deadlock streamt",
        "@{login} zwei deadlock-streamer, ein bot, das ist schon fast eine szene. die deutsche deadlock community bündelt genau die: wer offline geht, schickt seine leute weiter zum nächsten und wird selbst weitergereicht, wenn er live ist.",
    ),
    (
        "meine viewer schauen dann woanders wenn ich off bin",
        "@{login} klassisches problem, deine leute versickern sobald du offline gehst. das partner-netzwerk der deutschen deadlock community fängt genau das ab und leitet sie an den nächsten deutschen deadlock-streamer, du kriegst genauso raids zurück.",
    ),
    (
        "wie kriegt man hier eigentlich mehr leute rein",
        "@{login} ehrliche antwort: nicht durch den bot allein. die deutsche deadlock community hat ein partner-netzwerk, das raids zwischen deutschen deadlock-streamern hin und her schiebt, plus chat-schutz gegen spam und scam.",
    ),
    (
        "raids sind bei mir immer glückssache",
        "@{login} glückssache ist auch eine strategie, nur keine gute. bei der deutschen deadlock community laufen raids automatisch zwischen deutschen deadlock-streamern, wer offline geht schickt weiter, wer live ist bekommt.",
    ),
];

const PERIODIC_START: &[(&str, &str)] = &[
    (
        "chat redet über builds",
        "wer hier gerade über builds streitet: im community discord streiten sie seit monaten, mit besseren argumenten",
    ),
    (
        "ruhiger chat",
        "still hier. im discord der community ist es lauter, und man findet leute zum zocken",
    ),
    (
        "später abend, wenig los",
        "der stream läuft, der chat schläft. im community discord hängen um die uhrzeit meistens noch welche für ein paar runden",
    ),
    (
        "viele neue im chat",
        "ein paar neue gesichter heute, schön. wer mehr will als zuschauen, im community discord wird zusammen gezockt",
    ),
    (
        "hitziges thema im chat",
        "schöne diskussion hier, im community discord geht die genauso, nur mit mehr leuten die mitreden",
    ),
    (
        "chat fragt nach mitspielern",
        "wer mitspieler sucht, im discord der community stehen abends genug leute rum, die auch gerade niemanden zum stacken haben",
    ),
];

fn start_beispiele(pfad: PitchPfad) -> Vec<Beispiel> {
    let quelle = match pfad {
        PitchPfad::Anlass => ANLASS_START,
        PitchPfad::Partner => PARTNER_START,
        PitchPfad::Periodic => PERIODIC_START,
    };
    quelle
        .iter()
        .map(|(ausloeser, antwort)| Beispiel::aus(ausloeser, antwort))
        .collect()
}

pub async fn lade_gelernte(pool: &PgPool, pfad: PitchPfad) -> (Vec<Beispiel>, Vec<Beispiel>) {
    let gut = lade_bewertete(pool, pfad, "gut", MAX_GUT).await;
    let schlecht = lade_bewertete(pool, pfad, "schlecht", MAX_SCHLECHT).await;
    (gut, schlecht)
}

async fn lade_bewertete(
    pool: &PgPool,
    pfad: PitchPfad,
    bewertung: &str,
    limit: i64,
) -> Vec<Beispiel> {
    let rows = sqlx::query!(
        r#"SELECT trigger_text, generated_text
             FROM twitch_promo_pitch_log
            WHERE pfad = $1
              AND bewertung = $2
              AND generated_text IS NOT NULL
            ORDER BY COALESCE(bewertet_at, sent_at, created_at) DESC
            LIMIT $3"#,
        pfad.log_pfad(),
        bewertung,
        limit,
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    rows.into_iter()
        .filter_map(|row| {
            row.generated_text.map(|antwort| Beispiel {
                ausloeser: row.trigger_text,
                antwort,
            })
        })
        .collect()
}

pub fn baue_block(start: &[Beispiel], gut: &[Beispiel], schlecht: &[Beispiel]) -> String {
    let gute: Vec<&Beispiel> = if gut.len() >= MIN_LEARNED_GUT {
        gut.iter().take(MAX_GUT as usize).collect()
    } else {
        start.iter().take(MAX_GUT as usize).collect()
    };

    let mut zeilen = vec!["Gute Antworten (Ton und Länge nachahmen, Inhalt nicht wiederholen):".to_string()];
    for beispiel in gute {
        zeilen.push(render_zeile(beispiel));
    }
    if !schlecht.is_empty() {
        zeilen.push("So nicht (wurde als schlecht bewertet):".to_string());
        for beispiel in schlecht.iter().take(MAX_SCHLECHT as usize) {
            zeilen.push(render_zeile(beispiel));
        }
    }
    zeilen.join("\n")
}

fn render_zeile(beispiel: &Beispiel) -> String {
    match beispiel.ausloeser.as_deref().map(str::trim) {
        Some(ausloeser) if !ausloeser.is_empty() => {
            format!("- {} -> {}", ausloeser, beispiel.antwort.trim())
        }
        _ => format!("- {}", beispiel.antwort.trim()),
    }
}

pub async fn lade_block(pool: &PgPool, pfad: PitchPfad) -> String {
    let (gut, schlecht) = lade_gelernte(pool, pfad).await;
    baue_block(&start_beispiele(pfad), &gut, &schlecht)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lernbeispiel(n: usize) -> Beispiel {
        Beispiel {
            ausloeser: None,
            antwort: format!("gelernte antwort nummer {n}"),
        }
    }

    #[test]
    fn start_hat_sechs_je_pfad() {
        for pfad in [PitchPfad::Anlass, PitchPfad::Partner, PitchPfad::Periodic] {
            assert!(
                start_beispiele(pfad).len() >= 6,
                "{pfad:?} braucht mindestens sechs Startbeispiele"
            );
        }
    }

    #[test]
    fn unter_vier_gelernten_bleiben_startbeispiele() {
        let start = start_beispiele(PitchPfad::Anlass);
        let gut: Vec<Beispiel> = (0..3).map(lernbeispiel).collect();
        let block = baue_block(&start, &gut, &[]);
        assert!(block.contains(&start[0].antwort));
        assert!(!block.contains("gelernte antwort"));
    }

    #[test]
    fn ab_vier_gelernten_ersetzen_sie_start() {
        let start = start_beispiele(PitchPfad::Anlass);
        let gut: Vec<Beispiel> = (0..4).map(lernbeispiel).collect();
        let block = baue_block(&start, &gut, &[]);
        assert!(block.contains("gelernte antwort nummer 0"));
        assert!(!block.contains(&start[0].antwort));
    }

    #[test]
    fn schlecht_wird_gedeckelt_und_angezeigt() {
        let start = start_beispiele(PitchPfad::Partner);
        let schlecht: Vec<Beispiel> = (0..5)
            .map(|n| Beispiel {
                ausloeser: None,
                antwort: format!("schlechte antwort {n}"),
            })
            .collect();
        let block = baue_block(&start, &[], &schlecht);
        assert!(block.contains("So nicht"));
        assert!(block.contains("schlechte antwort 0"));
        assert!(!block.contains("schlechte antwort 3"));
    }
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn pool_in_schema(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query(
            r#"CREATE TABLE twitch_promo_pitch_log (
                id BIGSERIAL PRIMARY KEY,
                channel_login TEXT NOT NULL,
                target_user_id TEXT,
                pfad TEXT NOT NULL,
                occasion TEXT,
                trigger_text TEXT,
                generated_text TEXT,
                reject_reason TEXT,
                sent_at TIMESTAMPTZ,
                review_message_id BIGINT,
                bewertung TEXT,
                bewertet_at TIMESTAMPTZ,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    async fn insert_gut(pool: &PgPool, antwort: &str) {
        sqlx::query(
            "INSERT INTO twitch_promo_pitch_log
                (channel_login, pfad, generated_text, sent_at, bewertung, bewertet_at)
             VALUES ('kanal', 'anlass', $1, NOW(), 'gut', NOW())",
        )
        .bind(antwort)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn drei_gute_lassen_start_stehen_vier_ersetzen() {
        let Some(pool) = pool_in_schema("pitch_beispiele_test").await else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let start = start_beispiele(PitchPfad::Anlass);

        for n in 0..3 {
            insert_gut(&pool, &format!("gute gelernte antwort {n}")).await;
        }
        let (gut, schlecht) = lade_gelernte(&pool, PitchPfad::Anlass).await;
        assert_eq!(gut.len(), 3);
        let block = baue_block(&start, &gut, &schlecht);
        assert!(block.contains(&start[0].antwort));
        assert!(!block.contains("gute gelernte antwort"));

        insert_gut(&pool, "gute gelernte antwort 3").await;
        let (gut, schlecht) = lade_gelernte(&pool, PitchPfad::Anlass).await;
        assert_eq!(gut.len(), 4);
        let block = baue_block(&start, &gut, &schlecht);
        assert!(block.contains("gute gelernte antwort"));
        assert!(!block.contains(&start[0].antwort));
    }
}
