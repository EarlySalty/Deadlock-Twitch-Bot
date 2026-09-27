//! Transactional contest rules. All writes to one month use the same lock.
use super::*;

const ACTIVE_PARTNER_SQL: &str = "SELECT twitch_login FROM twitch_partners
        WHERE twitch_user_id=$1 AND departnered_at IS NULL AND admin_archived_at IS NULL
        AND COALESCE(status,'')='active' AND COALESCE(manual_partner_opt_out,0)=0
        AND COALESCE(technical_pause_reason,'')='' ORDER BY id DESC LIMIT 1 FOR SHARE";

#[derive(Deserialize, Default)]
pub struct CurrentQuery {
    #[serde(default)]
    offset: u32,
}

#[derive(Deserialize, Default)]
pub struct ArchiveQuery {
    before: Option<NaiveDate>,
}

async fn lock_month(
    tx: &mut Transaction<'_, Postgres>,
    month: NaiveDate,
) -> Result<(), sqlx::Error> {
    sqlx::query("SET LOCAL lock_timeout = '5s'")
        .execute(&mut **tx)
        .await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("clip-contest:{month}"))
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn database_now(tx: &mut Transaction<'_, Postgres>) -> Result<DateTime<Utc>, sqlx::Error> {
    sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut **tx)
        .await
}

fn phase_matches(expected: &ContestClock, actual: &ContestClock, phase: Phase) -> bool {
    expected.month == actual.month && actual.phase == phase
}

async fn begin_write(
    pool: &PgPool,
    expected: &ContestClock,
    phase: Phase,
) -> Result<Transaction<'static, Postgres>, Response> {
    let mut tx = pool.begin().await.map_err(|_| unavailable())?;
    lock_month(&mut tx, expected.month)
        .await
        .map_err(|_| unavailable())?;
    let actual = contest_clock(database_now(&mut tx).await.map_err(|_| unavailable())?);
    if !phase_matches(expected, &actual, phase) {
        return Err(json_error(
            StatusCode::CONFLICT,
            "phase_changed",
            "Die Monatsphase hat gewechselt. Bitte die Seite neu laden.",
        ));
    }
    sqlx::query(
        "INSERT INTO twitch_clip_contest_months (contest_month) VALUES ($1) ON CONFLICT DO NOTHING",
    )
    .bind(expected.month)
    .execute(&mut *tx)
    .await
    .map_err(|_| unavailable())?;
    let finalized: bool = sqlx::query_scalar(
        "SELECT finalized_at IS NOT NULL FROM twitch_clip_contest_months WHERE contest_month = $1",
    )
    .bind(expected.month)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| unavailable())?;
    if finalized {
        return Err(json_error(
            StatusCode::CONFLICT,
            "month_finalized",
            "Dieser Monat ist bereits abgeschlossen.",
        ));
    }
    Ok(tx)
}

async fn emit_effort_event(
    tx: &mut Transaction<'_, Postgres>,
    event_type: &str,
    channel_id: &str,
    channel_login: &str,
    source_id: &str,
    occurred_at: DateTime<Utc>,
    mut payload: Value,
) -> Result<(), sqlx::Error> {
    payload["partner_twitch_user_id"] = json!(channel_id);
    payload["schema_version"] = json!(1);
    sqlx::query(
        "INSERT INTO twitch_clip_contest_effort_outbox
        (event_type, partner_twitch_user_id, streamer_login, source_id, occurred_at, metadata)
        VALUES ($1,$2,LOWER($3),$4,$5,$6) ON CONFLICT (source_id) DO NOTHING",
    )
    .bind(event_type)
    .bind(channel_id)
    .bind(channel_login)
    .bind(source_id)
    .bind(occurred_at)
    .bind(payload)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn finalize_month(pool: &PgPool, month: NaiveDate) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    lock_month(&mut tx, month).await?;
    let now = database_now(&mut tx).await?;
    finalize_locked(&mut tx, month, now).await?;
    tx.commit().await
}

async fn finalize_locked(
    tx: &mut Transaction<'_, Postgres>,
    month: NaiveDate,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    if month >= contest_clock(now).month {
        return Ok(());
    }
    let finalized: Option<bool> = sqlx::query_scalar(
        "SELECT finalized_at IS NOT NULL FROM twitch_clip_contest_months WHERE contest_month=$1",
    )
    .bind(month)
    .fetch_optional(&mut **tx)
    .await?;
    if finalized != Some(false) {
        return Ok(());
    }
    let winners = sqlx::query(
        "SELECT s.id, s.broadcaster_twitch_id, s.broadcaster_login,
            COUNT(v.id)::bigint AS votes
        FROM twitch_clip_contest_submissions s
        LEFT JOIN twitch_clip_contest_votes v ON v.submission_id=s.id
        WHERE s.contest_month=$1 AND s.hidden_at IS NULL
        GROUP BY s.id ORDER BY COUNT(v.id) DESC, s.submitted_at ASC, s.id ASC LIMIT 3",
    )
    .bind(month)
    .fetch_all(&mut **tx)
    .await?;
    let next = next_month(month);
    // Stable event time, including a delayed restart: awards belong to the result day.
    let result_at = Berlin
        .with_ymd_and_hms(next.year(), next.month(), 1, 0, 0, 0)
        .single()
        .expect("month boundary")
        .with_timezone(&Utc);
    for (index, row) in winners.iter().enumerate() {
        let rank = (index + 1) as i16;
        let id: i64 = row.try_get("id")?;
        let votes: i64 = row.try_get("votes")?;
        let channel_id: String = row.try_get("broadcaster_twitch_id")?;
        let login: String = row.try_get("broadcaster_login")?;
        sqlx::query("INSERT INTO twitch_clip_contest_hall_of_fame (contest_month, rank, submission_id, vote_count)
            VALUES ($1,$2,$3,$4)")
            .bind(month).bind(rank).bind(id).bind(i32::try_from(votes).unwrap_or(i32::MAX))
            .execute(&mut **tx).await?;
        emit_effort_event(
            tx,
            "clip_top3",
            &channel_id,
            &login,
            &format!("clip-contest:top3:{month}:{rank}:{id}"),
            result_at,
            json!({"contest_month":month,"rank":rank,"submission_id":id,"votes":votes}),
        )
        .await?;
    }
    // Also seal an empty/all-hidden month, so restoring a clip never creates late winners.
    sqlx::query("UPDATE twitch_clip_contest_months SET finalized_at=$2 WHERE contest_month=$1 AND finalized_at IS NULL")
        .bind(month).bind(now).execute(&mut **tx).await?;
    Ok(())
}

async fn finalize_pending(pool: &PgPool) -> Result<(), sqlx::Error> {
    let now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(pool)
        .await?;
    let current = contest_clock(now).month;
    sqlx::query(
        "INSERT INTO twitch_clip_contest_months (contest_month) VALUES ($1) ON CONFLICT DO NOTHING",
    )
    .bind(current)
    .execute(pool)
    .await?;
    let months: Vec<NaiveDate> = sqlx::query_scalar(
        "SELECT contest_month FROM twitch_clip_contest_months
        WHERE contest_month < $1 AND finalized_at IS NULL ORDER BY contest_month",
    )
    .bind(current)
    .fetch_all(pool)
    .await?;
    for month in months {
        finalize_month(pool, month).await?;
    }
    Ok(())
}

pub async fn finalize_due_months(pool: &PgPool) {
    if let Err(error) = finalize_pending(pool).await {
        tracing::warn!(%error, "Clip-Wettbewerb: Monatsabschluss fehlgeschlagen");
    }
}

fn is_own(row: &PgRow, aliases: &[String]) -> bool {
    let key: String = row.try_get("submitter_person_key").unwrap_or_default();
    let saved: Vec<String> = row.try_get("submitter_aliases").unwrap_or_default();
    let provider: String = row.try_get("submitter_provider").unwrap_or_default();
    let user: String = row.try_get("submitter_user_id").unwrap_or_default();
    let broadcaster: String = row.try_get("broadcaster_twitch_id").unwrap_or_default();
    let creator: Option<String> = row.try_get("creator_twitch_id").unwrap_or(None);
    aliases.iter().any(|alias| {
        alias == &key
            || saved.contains(alias)
            || alias == &format!("{provider}:{user}")
            || alias == &format!("twitch:{broadcaster}")
            || creator
                .as_ref()
                .is_some_and(|creator| alias == &format!("twitch:{creator}"))
    })
}

fn clip_json(row: &PgRow, aliases: &[String]) -> Value {
    json!({
        "id": row.get::<i64,_>("id"),
        "clip_id": row.get::<String,_>("twitch_clip_id"),
        "clip_url": row.get::<String,_>("clip_url"),
        "thumbnail_url": row.get::<Option<String>,_>("clip_thumbnail_url"),
        "title": row.get::<String,_>("clip_title"),
        "channel": row.get::<String,_>("broadcaster_login"),
        "channel_name": row.get::<Option<String>,_>("broadcaster_name"),
        "votes": row.get::<i64,_>("votes"),
        "my_vote": row.try_get::<bool,_>("my_vote").unwrap_or(false),
        "my_own": is_own(row, aliases),
        "submitted_at": row.get::<DateTime<Utc>,_>("submitted_at"),
    })
}

async fn current_rows(
    pool: &PgPool,
    month: NaiveDate,
    discord: Option<&str>,
    aliases: &[String],
    offset: i64,
    limit: i64,
) -> Result<Vec<Value>, sqlx::Error> {
    let rows = sqlx::query("SELECT s.*, COUNT(v.id)::bigint AS votes,
        COALESCE(BOOL_OR(v.voter_discord_id=$2),FALSE) AS my_vote
        FROM twitch_clip_contest_submissions s LEFT JOIN twitch_clip_contest_votes v ON v.submission_id=s.id
        WHERE s.contest_month=$1 AND s.hidden_at IS NULL GROUP BY s.id
        ORDER BY COUNT(v.id) DESC, s.submitted_at ASC, s.id ASC LIMIT $3 OFFSET $4")
        .bind(month).bind(discord.unwrap_or("")).bind(limit).bind(offset).fetch_all(pool).await?;
    Ok(rows.iter().map(|row| clip_json(row, aliases)).collect())
}

pub async fn current_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    state: Option<Extension<DashboardAuthState>>,
    headers: HeaderMap,
    Query(query): Query<CurrentQuery>,
) -> Response {
    let clock = contest_clock(Utc::now());
    let identity = submit_identity(&pool, &headers, &auth, state.as_ref().map(|e| &e.0)).await;
    let discord = identity
        .as_ref()
        .filter(|i| i.provider == "discord")
        .map(|i| i.user_id.as_str());
    let aliases = identity
        .as_ref()
        .map(|i| i.aliases.as_slice())
        .unwrap_or(&[]);
    let rows = match current_rows(
        &pool,
        clock.month,
        discord,
        aliases,
        i64::from(query.offset),
        24,
    )
    .await
    {
        Ok(rows) => rows,
        Err(_) => return unavailable(),
    };
    let top3 = match current_rows(&pool, clock.month, discord, aliases, 0, 3).await {
        Ok(rows) => rows,
        Err(_) => return unavailable(),
    };
    let total = match sqlx::query_scalar::<_,i64>("SELECT COUNT(*) FROM twitch_clip_contest_submissions WHERE contest_month=$1 AND hidden_at IS NULL")
        .bind(clock.month).fetch_one(&pool).await { Ok(total) => total, Err(_) => return unavailable() };
    let next_offset = i64::from(query.offset) + rows.len() as i64;
    no_store_json(
        json!({"month":clock.month,"month_label":month_label(clock.month),
        "phase":clock.phase.as_str(),"phase_ends_at":clock.phase_ends_at,
        "submissions":rows,"top3":top3,"total":total,
        "next_offset":(next_offset < total).then_some(next_offset)}),
    )
}

pub async fn archive_handler(
    State(pool): State<PgPool>,
    Query(query): Query<ArchiveQuery>,
) -> Response {
    if finalize_pending(&pool).await.is_err() {
        return unavailable();
    }
    let months: Vec<NaiveDate> = match sqlx::query_scalar(
        "SELECT contest_month FROM twitch_clip_contest_months
        WHERE finalized_at IS NOT NULL AND ($1::date IS NULL OR contest_month < $1)
        ORDER BY contest_month DESC LIMIT 24",
    )
    .bind(query.before)
    .fetch_all(&pool)
    .await
    {
        Ok(rows) => rows,
        Err(_) => return unavailable(),
    };
    let rows = match sqlx::query("SELECT s.*,h.contest_month,h.rank,h.vote_count::bigint AS votes
        FROM twitch_clip_contest_hall_of_fame h JOIN twitch_clip_contest_submissions s ON s.id=h.submission_id
        WHERE h.contest_month=ANY($1) AND s.hidden_at IS NULL ORDER BY h.contest_month DESC,h.rank")
        .bind(&months).fetch_all(&pool).await { Ok(rows) => rows, Err(_) => return unavailable() };
    let mut grouped: BTreeMap<NaiveDate, Vec<Value>> =
        months.iter().map(|m| (*m, Vec::new())).collect();
    for row in &rows {
        let mut clip = clip_json(row, &[]);
        clip["rank"] = json!(row.get::<i16, _>("rank"));
        grouped
            .entry(row.get("contest_month"))
            .or_default()
            .push(clip);
    }
    let next_before = (months.len() == 24)
        .then(|| months.last().copied())
        .flatten();
    let months: Vec<Value> = grouped.into_iter().rev().map(|(month,winners)|
        json!({"month":month,"month_label":month_label(month),"winners":winners})).collect();
    no_store_json(json!({"months":months,"next_before":next_before}))
}

async fn submissions_used(
    pool: &PgPool,
    month: NaiveDate,
    aliases: &[String],
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM twitch_clip_contest_submissions WHERE contest_month=$1
        AND (submitter_person_key=ANY($2) OR submitter_aliases && $2)",
    )
    .bind(month)
    .bind(aliases)
    .fetch_one(pool)
    .await
}

pub async fn session_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    state: Option<Extension<DashboardAuthState>>,
    headers: HeaderMap,
) -> Response {
    let clock = contest_clock(Utc::now());
    let identity = submit_identity(&pool, &headers, &auth, state.as_ref().map(|e| &e.0)).await;
    let discord = identity.as_ref().filter(|i| i.provider == "discord");
    let eligibility = match discord {
        Some(i) => discord_eligibility(&i.user_id).await.ok(),
        None => None,
    };
    let votes_used = if let Some(i) = discord {
        match sqlx::query_scalar::<_,i64>("SELECT COUNT(*) FROM twitch_clip_contest_votes WHERE contest_month=$1 AND voter_discord_id=$2")
            .bind(clock.month).bind(&i.user_id).fetch_one(&pool).await { Ok(n) => n, Err(_) => return unavailable() }
    } else {
        0
    };
    let submitted = if let Some(i) = &identity {
        match submissions_used(&pool, clock.month, &i.aliases).await {
            Ok(n) => n,
            Err(_) => return unavailable(),
        }
    } else {
        0
    };
    let eligible = eligibility
        .as_ref()
        .is_some_and(|e| e.present && e.account_age_ok && e.member_age_ok);
    no_store_json(json!({
        "authenticated":identity.is_some(),"provider":identity.as_ref().map(|i| i.provider),
        "display_name":identity.as_ref().map(|i| &i.display_name),
        "discord_authenticated":discord.is_some(),"is_admin":auth.is_privileged(),
        "can_submit":identity.is_some() && clock.phase == Phase::Submission && submitted < MAX_SUBMISSIONS_PER_MONTH,
        "submissions_used":submitted,"submissions_limit":MAX_SUBMISSIONS_PER_MONTH,
        "can_vote":eligible && clock.phase == Phase::Voting && votes_used < MAX_VOTES_PER_MONTH,
        "votes_used":votes_used,"votes_limit":MAX_VOTES_PER_MONTH,
        "eligibility_unavailable":discord.is_some() && eligibility.is_none(),
        "discord_eligibility":eligibility.map(|e| json!({"account_age_ok":e.account_age_ok,
            "member_age_ok":e.member_age_ok,"present":e.present,"joined_at":e.joined_at})),
    }))
}

fn validate_clip_age(clip: &HelixClip, now: DateTime<Utc>) -> Result<(), Response> {
    if clip.created_at > now || clip.created_at < now - Duration::days(MAX_CLIP_AGE_DAYS) {
        return Err(json_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "clip_too_old",
            "Der Clip darf höchstens 60 Tage alt sein und nicht in der Zukunft liegen.",
        ));
    }
    Ok(())
}

async fn persist_submission(
    tx: &mut Transaction<'_, Postgres>,
    month: NaiveDate,
    identity: &SubmitIdentity,
    clip: &HelixClip,
    channel: &str,
    clip_db_id: i64,
) -> Result<i64, Response> {
    let used: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM twitch_clip_contest_submissions WHERE contest_month=$1
        AND (submitter_person_key=ANY($2) OR submitter_aliases && $2)",
    )
    .bind(month)
    .bind(&identity.aliases)
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| unavailable())?;
    if used >= MAX_SUBMISSIONS_PER_MONTH {
        return Err(json_error(
            StatusCode::TOO_MANY_REQUESTS,
            "submission_limit",
            "Du hast diesen Monat bereits drei Clips eingereicht.",
        ));
    }
    let row: Option<(i64,DateTime<Utc>)> = sqlx::query_as("INSERT INTO twitch_clip_contest_submissions
        (contest_month,clip_db_id,twitch_clip_id,clip_url,clip_title,clip_thumbnail_url,
        broadcaster_twitch_id,broadcaster_login,broadcaster_name,creator_twitch_id,game_id,clip_created_at,
        submitter_provider,submitter_user_id,submitter_person_key,submitter_aliases,submitter_display_name,submission_slot)
        VALUES ($1,$2,$3,$4,$5,$6,$7,LOWER($8),$9,$10,$11,$12,$13,$14,$15,$16,$17,$18)
        ON CONFLICT (contest_month,twitch_clip_id) DO NOTHING RETURNING id,submitted_at")
        .bind(month).bind(clip_db_id).bind(&clip.clip_id).bind(&clip.url).bind(&clip.title).bind(&clip.thumbnail_url)
        .bind(&clip.broadcaster_id).bind(channel).bind(&clip.broadcaster_name).bind(&clip.creator_id)
        .bind(&clip.game_id).bind(clip.created_at).bind(identity.provider).bind(&identity.user_id)
        .bind(&identity.person_key).bind(&identity.aliases).bind(&identity.display_name).bind((used+1) as i16)
        .fetch_optional(&mut **tx).await.map_err(|_| unavailable())?;
    let (id, submitted_at) = row.ok_or_else(|| {
        json_error(
            StatusCode::CONFLICT,
            "clip_already_submitted",
            "Dieser Clip ist in diesem Monat bereits eingereicht.",
        )
    })?;
    emit_effort_event(
        tx,
        "clip_submitted",
        &clip.broadcaster_id,
        channel,
        &format!("clip-contest:submission:{id}"),
        submitted_at,
        json!({"contest_month":month,"submission_id":id,"twitch_clip_id":clip.clip_id}),
    )
    .await
    .map_err(|_| unavailable())?;
    Ok(id)
}

pub async fn submit_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    state: Option<Extension<DashboardAuthState>>,
    headers: HeaderMap,
    Json(body): Json<SubmitBody>,
) -> Response {
    if !valid_write_origin(&headers) {
        return json_error(
            StatusCode::FORBIDDEN,
            "invalid_csrf",
            "Diese Anfrage wurde abgelehnt.",
        );
    }
    let clock = contest_clock(Utc::now());
    if clock.phase != Phase::Submission {
        return json_error(
            StatusCode::CONFLICT,
            "submission_closed",
            "Einreichen ist vom 1. bis einschließlich 21. möglich.",
        );
    }
    let Some(identity) =
        submit_identity(&pool, &headers, &auth, state.as_ref().map(|e| &e.0)).await
    else {
        return json_error(
            StatusCode::UNAUTHORIZED,
            "login_required",
            "Zum Einreichen musst du mit Discord oder Twitch angemeldet sein.",
        );
    };
    let Some(clip_id) = clip_id_from_url(&body.clip_url) else {
        return json_error(
            StatusCode::BAD_REQUEST,
            "invalid_clip_url",
            "Bitte gib eine gültige Twitch-Clip-Adresse ein.",
        );
    };
    let clip = match fetch_helix_clip(&clip_id).await {
        Ok(clip) => clip,
        Err(response) => return response,
    };
    if let Err(response) = validate_clip_age(&clip, Utc::now()) {
        return response;
    }
    let game = match deadlock_game_id(&pool).await {
        Ok(game) => game,
        Err(response) => return response,
    };
    if clip.game_id != game {
        return json_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "wrong_game",
            "Der Clip muss aus einem Deadlock-Stream stammen.",
        );
    }
    // Preflight before the shared repository, without occupying a transaction connection.
    let channel: Option<String> = match sqlx::query_scalar(ACTIVE_PARTNER_SQL)
        .bind(&clip.broadcaster_id)
        .fetch_optional(&pool)
        .await
    {
        Ok(channel) => channel,
        Err(_) => return unavailable(),
    };
    let Some(channel) = channel else {
        return json_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "channel_not_active_partner",
            "Der Clip muss aus einem aktuell aktiven Partnerkanal stammen.",
        );
    };
    // Check quota before registering a new shared clip. A hidden clip still uses a slot.
    match submissions_used(&pool, clock.month, &identity.aliases).await {
        Ok(n) if n < MAX_SUBMISSIONS_PER_MONTH => {}
        Ok(_) => {
            return json_error(
                StatusCode::TOO_MANY_REQUESTS,
                "submission_limit",
                "Du hast diesen Monat bereits drei Clips eingereicht.",
            )
        }
        Err(_) => return unavailable(),
    }
    let repository = ClipRepository::new(pool.clone());
    if repository
        .ensure_monitored_streamer(&channel, &clip.broadcaster_id)
        .await
        .is_err()
    {
        return unavailable();
    }
    let record = ClipRecord {
        clip_id: clip.clip_id.clone(),
        clip_url: clip.url.clone(),
        clip_title: clip.title.clone(),
        thumbnail_url: clip.thumbnail_url.clone(),
        streamer_login: channel.clone(),
        twitch_user_id: clip.broadcaster_id.clone(),
        broadcaster_name: Some(clip.broadcaster_name.clone()),
        created_at: clip.created_at.to_rfc3339(),
        duration_seconds: clip.duration_seconds,
        view_count: clip.view_count,
        game_name: Some("Deadlock".into()),
        game_id: Some(clip.game_id.clone()),
        vod_id: clip.vod_id.clone(),
        vod_offset_s: clip.vod_offset_s,
    };
    let clip_db_id = match repository.register_clip(&record).await {
        Ok((id, _)) => id,
        // The social fetcher may have registered the same clip concurrently.
        Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
            match sqlx::query_scalar::<_, i64>(
                "SELECT id FROM twitch_clips_social_media WHERE clip_id=$1",
            )
            .bind(&clip.clip_id)
            .fetch_one(&pool)
            .await
            {
                Ok(id) => id,
                Err(_) => return unavailable(),
            }
        }
        Err(_) => return unavailable(),
    };
    let mut tx = match begin_write(&pool, &clock, Phase::Submission).await {
        Ok(tx) => tx,
        Err(response) => return response,
    };
    // No secondary pool checkout while holding the month lock. Revalidate the
    // partner under a row lock because status or login may have changed during I/O.
    let channel: String = match sqlx::query_scalar(ACTIVE_PARTNER_SQL)
        .bind(&clip.broadcaster_id)
        .fetch_optional(&mut *tx)
        .await
    {
        Ok(Some(channel)) => channel,
        Ok(None) => {
            return json_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "channel_not_active_partner",
                "Der Quellkanal ist nicht mehr als aktiver Partner freigegeben.",
            )
        }
        Err(_) => return unavailable(),
    };
    // Recheck after shared repository I/O, before the contest/outbox write.
    let actual = match database_now(&mut tx).await {
        Ok(now) => contest_clock(now),
        Err(_) => return unavailable(),
    };
    if let Err(response) = validate_clip_age(
        &clip,
        match database_now(&mut tx).await {
            Ok(now) => now,
            Err(_) => return unavailable(),
        },
    ) {
        return response;
    }
    if !phase_matches(&clock, &actual, Phase::Submission) {
        return json_error(
            StatusCode::CONFLICT,
            "phase_changed",
            "Die Einreichungsphase ist gerade zu Ende gegangen.",
        );
    }
    let id = match persist_submission(&mut tx, clock.month, &identity, &clip, &channel, clip_db_id)
        .await
    {
        Ok(id) => id,
        Err(response) => return response,
    };
    if tx.commit().await.is_err() {
        return unavailable();
    }
    no_store_json(json!({"ok":true,"submission_id":id,"message":"Clip eingereicht."}))
}

async fn persist_vote(
    tx: &mut Transaction<'_, Postgres>,
    month: NaiveDate,
    discord_id: &str,
    aliases: &[String],
    id: i64,
) -> Result<i64, Response> {
    let row = sqlx::query("SELECT * FROM twitch_clip_contest_submissions WHERE id=$1 AND contest_month=$2 AND hidden_at IS NULL")
        .bind(id).bind(month).fetch_optional(&mut **tx).await.map_err(|_| unavailable())?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND,"submission_not_found","Dieser Clip steht nicht zur Abstimmung."))?;
    if is_own(&row, aliases) {
        return Err(json_error(
            StatusCode::FORBIDDEN,
            "own_clip",
            "Für deinen eigenen Clip kannst du nicht abstimmen.",
        ));
    }
    let already: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM twitch_clip_contest_votes WHERE submission_id=$1 AND voter_discord_id=$2)")
        .bind(id).bind(discord_id).fetch_one(&mut **tx).await.map_err(|_| unavailable())?;
    if already {
        return Err(json_error(
            StatusCode::CONFLICT,
            "already_voted",
            "Für diesen Clip hast du bereits abgestimmt.",
        ));
    }
    let used: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_clip_contest_votes WHERE contest_month=$1 AND voter_discord_id=$2")
        .bind(month).bind(discord_id).fetch_one(&mut **tx).await.map_err(|_| unavailable())?;
    if used >= MAX_VOTES_PER_MONTH {
        return Err(json_error(
            StatusCode::TOO_MANY_REQUESTS,
            "vote_limit",
            "Du hast deine fünf Stimmen für diesen Monat bereits vergeben.",
        ));
    }
    sqlx::query("INSERT INTO twitch_clip_contest_votes (submission_id,voter_discord_id,contest_month,vote_slot) VALUES ($1,$2,$3,$4)")
        .bind(id).bind(discord_id).bind(month).bind((used+1) as i16).execute(&mut **tx).await.map_err(|_| unavailable())?;
    Ok(used + 1)
}

pub async fn vote_handler(
    State(pool): State<PgPool>,
    state: Option<Extension<DashboardAuthState>>,
    headers: HeaderMap,
    Json(body): Json<VoteBody>,
) -> Response {
    if !valid_write_origin(&headers) {
        return json_error(
            StatusCode::FORBIDDEN,
            "invalid_csrf",
            "Diese Anfrage wurde abgelehnt.",
        );
    }
    let clock = contest_clock(Utc::now());
    if clock.phase != Phase::Voting {
        return json_error(
            StatusCode::CONFLICT,
            "voting_closed",
            "Abgestimmt wird vom 22. bis zum Monatsende.",
        );
    }
    let Some(discord) = discord_session(state.as_ref().map(|e| &e.0), &headers).await else {
        return json_error(
            StatusCode::UNAUTHORIZED,
            "discord_required",
            "Zum Abstimmen musst du mit Discord angemeldet sein.",
        );
    };
    let eligibility = match discord_eligibility(&discord.user_id).await {
        Ok(e) => e,
        Err(response) => return response,
    };
    if !eligibility.account_age_ok {
        return json_error(
            StatusCode::FORBIDDEN,
            "account_too_new",
            "Dein Discord-Konto muss mindestens 30 Tage alt sein.",
        );
    }
    if !eligibility.present || !eligibility.member_age_ok {
        return json_error(
            StatusCode::FORBIDDEN,
            "member_too_new",
            "Du musst seit mindestens 7 Tagen Mitglied der Community sein.",
        );
    }
    let aliases = match identity_aliases(&pool, "discord", &discord.user_id).await {
        Ok(a) => a,
        Err(_) => return unavailable(),
    };
    let mut tx = match begin_write(&pool, &clock, Phase::Voting).await {
        Ok(tx) => tx,
        Err(response) => return response,
    };
    let used = match persist_vote(
        &mut tx,
        clock.month,
        &discord.user_id,
        &aliases,
        body.submission_id,
    )
    .await
    {
        Ok(n) => n,
        Err(response) => return response,
    };
    if tx.commit().await.is_err() {
        return unavailable();
    }
    no_store_json(
        json!({"ok":true,"votes_used":used,"votes_left":MAX_VOTES_PER_MONTH-used,"message":"Stimme gespeichert."}),
    )
}

pub async fn admin_submissions_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Response {
    if !auth.is_privileged() {
        return json_error(
            StatusCode::FORBIDDEN,
            "admin_required",
            "Diese Aktion ist nur für Admins verfügbar.",
        );
    }
    let rows = match sqlx::query(
        "SELECT s.*,COUNT(v.id)::bigint AS votes FROM twitch_clip_contest_submissions s
        LEFT JOIN twitch_clip_contest_votes v ON v.submission_id=s.id WHERE s.hidden_at IS NOT NULL
        GROUP BY s.id ORDER BY s.hidden_at DESC LIMIT 200",
    )
    .fetch_all(&pool)
    .await
    {
        Ok(rows) => rows,
        Err(_) => return unavailable(),
    };
    let clips: Vec<Value> = rows
        .iter()
        .map(|row| {
            let mut clip = clip_json(row, &[]);
            clip["month"] = json!(row.get::<NaiveDate, _>("contest_month"));
            clip
        })
        .collect();
    no_store_json(json!({"submissions":clips}))
}

pub async fn hide_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Json(body): Json<HideBody>,
) -> Response {
    if !auth.is_privileged() {
        return json_error(
            StatusCode::FORBIDDEN,
            "admin_required",
            "Diese Aktion ist nur für Admins verfügbar.",
        );
    }
    if !valid_write_origin(&headers) {
        return json_error(
            StatusCode::FORBIDDEN,
            "invalid_csrf",
            "Diese Anfrage wurde abgelehnt.",
        );
    }
    if body.reason.chars().count() > 500 {
        return json_error(
            StatusCode::BAD_REQUEST,
            "reason_too_long",
            "Die Begründung darf höchstens 500 Zeichen haben.",
        );
    }
    let actor = match auth {
        DashboardAuthLevel::Admin { actor: Some(actor) } => {
            format!("twitch:{}", actor.twitch_user_id)
        }
        _ => "authenticated-admin".into(),
    };
    let month: NaiveDate = match sqlx::query_scalar(
        "SELECT contest_month FROM twitch_clip_contest_submissions WHERE id=$1",
    )
    .bind(body.submission_id)
    .fetch_optional(&pool)
    .await
    {
        Ok(Some(month)) => month,
        Ok(None) => {
            return json_error(
                StatusCode::NOT_FOUND,
                "submission_not_found",
                "Der Clip wurde nicht gefunden.",
            )
        }
        Err(_) => return unavailable(),
    };
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return unavailable(),
    };
    if lock_month(&mut tx, month).await.is_err() {
        return unavailable();
    }
    let now = match database_now(&mut tx).await {
        Ok(now) => now,
        Err(_) => return unavailable(),
    };
    // Recheck under the shared month lock, including a request that waited across midnight.
    if finalize_locked(&mut tx, month, now).await.is_err() {
        return unavailable();
    }
    let result = sqlx::query(
        "UPDATE twitch_clip_contest_submissions SET
        hidden_at=CASE WHEN $2 THEN clock_timestamp() ELSE NULL END,
        hidden_by=CASE WHEN $2 THEN $3 ELSE NULL END
        WHERE id=$1 AND (hidden_at IS NOT NULL) IS DISTINCT FROM $2",
    )
    .bind(body.submission_id)
    .bind(body.hidden)
    .bind(&actor)
    .execute(&mut *tx)
    .await;
    match result {
        Ok(result) if result.rows_affected()>0 => {
            if sqlx::query("INSERT INTO twitch_clip_contest_moderation (submission_id,actor,hidden,reason) VALUES ($1,$2,$3,$4)")
                .bind(body.submission_id).bind(actor).bind(body.hidden).bind(body.reason.trim())
                .execute(&mut *tx).await.is_err() { return unavailable(); }
        },
        Ok(_) => {}, Err(_) => return unavailable(),
    }
    if tx.commit().await.is_err() {
        return unavailable();
    }
    no_store_json(json!({"ok":true,"hidden":body.hidden,"votes_preserved":true}))
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
