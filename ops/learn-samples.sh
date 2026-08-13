#!/usr/bin/env bash
# Sichtung der Reaktions-Samples aus dem Engagement-Lernmodus.
#
# Zeigt, worauf im Stream reagiert wurde und was daraufhin geschrieben wurde,
# und erlaubt ein Urteil pro Sample. Als `bad` markierte Samples fallen aus dem
# Few-Shot-Stil und aus der Profil-Destillation heraus.
#
#   ops/learn-samples.sh list [ANZAHL]     letzte Samples (Default 20)
#   ops/learn-samples.sh show ID           ein Sample mit vollem Kontext
#   ops/learn-samples.sh good ID [ID...]   als Vorbild markieren
#   ops/learn-samples.sh bad ID [ID...]    aus dem Lernmaterial nehmen
#   ops/learn-samples.sh reset ID [ID...]  Urteil zurücknehmen
#   ops/learn-samples.sh stats             Fortschritt der Lernphase
#   ops/learn-samples.sh profile           aktuelles destilliertes Profil
#
# DATABASE_URL muss gesetzt sein (oder PG*-Variablen für psql).
set -euo pipefail

psql_run() {
    if [[ -n "${DATABASE_URL:-}" ]]; then
        psql "$DATABASE_URL" -v ON_ERROR_STOP=1 "$@"
    else
        psql -v ON_ERROR_STOP=1 "$@"
    fi
}

# Setzt das Urteil für eine Liste von IDs. $1 = 'good' | 'bad' | NULL-Literal.
set_verdict() {
    local verdict="$1"
    shift
    [[ $# -gt 0 ]] || { echo "IDs fehlen" >&2; exit 1; }
    local ids
    ids=$(printf '%s,' "$@")
    ids=${ids%,}
    psql_run -c "UPDATE twitch_engagement_reaction_samples
                    SET verdict = ${verdict}
                  WHERE id IN (${ids})"
}

cmd=${1:-list}
shift || true

case "$cmd" in
list)
    limit=${1:-20}
    psql_run -P pager=off -c "
        SELECT id,
               to_char(message_ts, 'DD.MM HH24:MI') AS zeit,
               channel_login AS kanal,
               COALESCE(verdict, '-') AS urteil,
               CASE WHEN has_stream_context THEN 'ja' ELSE 'nein' END AS audio,
               left(my_message, 60) AS nachricht
          FROM twitch_engagement_reaction_samples
         ORDER BY message_ts DESC
         LIMIT ${limit}"
    ;;
show)
    id=${1:?ID fehlt}
    # -A -t: unformatiert, damit psql die mehrzeiligen Kontexte nicht in eine
    # Tabellenzelle mit Fortsetzungszeichen presst.
    psql_run -P pager=off -A -t -c "
        SELECT E'\n=== Sample ' || id || ' | ' || channel_login || ' | '
               || to_char(message_ts, 'DD.MM.YYYY HH24:MI:SS') || ' | Urteil: '
               || COALESCE(verdict, 'ungesichtet') || E' ===\n\n'
               || E'--- Stream-Audio (Sekunden relativ zur Nachricht) ---\n'
               || COALESCE(NULLIF(stream_context, ''), '(keine Aufnahme)')
               || E'\n\n--- Chat davor ---\n'
               || COALESCE(NULLIF(chat_context, ''), '(nichts)')
               || E'\n\n--- Er schreibt ---\n' || my_message || E'\n'
          FROM twitch_engagement_reaction_samples
         WHERE id = ${id}"
    ;;
good) set_verdict "'good'" "$@" ;;
bad) set_verdict "'bad'" "$@" ;;
reset) set_verdict "NULL" "$@" ;;
stats)
    psql_run -P pager=off -c "
        SELECT count(*) AS samples,
               count(*) FILTER (WHERE has_stream_context) AS mit_audio,
               count(*) FILTER (WHERE verdict = 'good') AS gut,
               count(*) FILTER (WHERE verdict = 'bad') AS schlecht,
               count(*) FILTER (WHERE verdict IS NULL) AS ungesichtet,
               count(DISTINCT channel_login) AS kanaele,
               to_char(min(message_ts), 'DD.MM HH24:MI') AS erstes,
               to_char(max(message_ts), 'DD.MM HH24:MI') AS letztes
          FROM twitch_engagement_reaction_samples"
    psql_run -P pager=off -c "
        SELECT channel_login AS kanal, count(*) AS samples,
               to_char(max(message_ts), 'DD.MM HH24:MI') AS zuletzt
          FROM twitch_engagement_reaction_samples
         GROUP BY channel_login ORDER BY count(*) DESC LIMIT 15"
    ;;
profile)
    psql_run -P pager=off -A -t -c "
        SELECT E'Stand ' || to_char(created_at, 'DD.MM.YYYY HH24:MI') || E':\n\n' || content
          FROM twitch_engagement_soul
         WHERE kind = 'reaction_profile'
         ORDER BY created_at DESC LIMIT 1"
    ;;
*)
    sed -n '2,17p' "$0" | sed 's/^# \{0,1\}//'
    exit 1
    ;;
esac
