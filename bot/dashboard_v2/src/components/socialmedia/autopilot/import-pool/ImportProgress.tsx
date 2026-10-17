import { useLanguage, useT } from "@/context/LanguageContext";
import {
  activeImport,
  filterDescription,
  importStatusLabel,
  localTimestamp,
} from "./presentation";
import type { ImportJob } from "./types";

export function ImportProgress({ job }: { job: ImportJob }) {
  const t = useT();
  const { locale } = useLanguage();
  const incomplete =
    job.incomplete_windows.length > 0 || job.status === "incomplete";
  const problem = incomplete || job.status === "error";
  const visibleStatus = problem || activeImport(job);
  const status = (
    <p
      className={problem ? "import-pool-warning" : "import-pool-progress-title"}
      role="status"
    >
      {t(importStatusLabel(job))}
    </p>
  );
  return (
    <div className="import-pool-progress" data-import-status={job.status}>
      {visibleStatus && status}
      {job.next_retry_at && (
        <p>
          {t("Nächster Versuch frühestens")}:{" "}
          {localTimestamp(job.next_retry_at, job.timezone, locale)}
        </p>
      )}
      {job.last_error && (
        <p className="import-pool-warning">{t(job.last_error)}</p>
      )}
      <details className="import-pool-summary">
      <summary>{t("{count} erfasst", { count: job.persisted_count })} · {t("Importdetails")}</summary>
      {!visibleStatus && status}
      <p className="import-pool-help">
        {t("Verwendet den gespeicherten Zeitraum. Ein laufender Import behält seinen ursprünglichen Zeitraum.")}
      </p>
      <p className="import-pool-help">
        {t("Festgelegter Zeitraum")}:{" "}
        {filterDescription(job.clip_date_filter, t)}. {job.timezone}
      </p>
      {job.clip_date_filter.kind === "all" ? <p className="import-pool-help">{t("Alle verfügbaren Clips")}</p> : <p className="import-pool-help">
        {t("Erfassung von")} {localTimestamp(job.started_at, job.timezone, locale)}{" "}
        {t("bis")} {localTimestamp(job.ended_at, job.timezone, locale)} (
        {t("Endzeitpunkt ausgeschlossen")}).
      </p>}
      <dl className="import-pool-counts">
        <div>
          <dt>{t("Neu im Pool gespeichert")}</dt>
          <dd>{job.persisted_count}</dd>
        </div>
        <div>
          <dt>{t("Von Twitch abgerufen")}</dt>
          <dd>{job.raw_fetched}</dd>
        </div>
        <div>
          <dt>{t("Passende Abrufe")}</dt>
          <dd>{job.eligible_fetched}</dd>
        </div>
        <div>
          <dt>{t("Abgeschlossene Zeitfenster")}</dt>
          <dd>{job.completed_windows}</dd>
        </div>
        <div>
          <dt>{t("Ausstehende Zeitfenster")}</dt>
          <dd>{job.pending_windows}</dd>
        </div>
      </dl>
      <p className="import-pool-help">
        {t(
          "Abrufzahlen können Wiederholungen enthalten. Neu gespeicherte Pool-Einträge werden getrennt gezählt.",
        )}
      </p>
      {job.active_window && (
        <p className="import-pool-help">
          {t("Aktives Zeitfenster")}:{" "}
          {localTimestamp(job.active_window.start, job.timezone, locale)} {t("bis")}{" "}
          {localTimestamp(job.active_window.end, job.timezone, locale)}.{" "}
          {job.active_window.pages} {t("Seiten abgerufen")}.
        </p>
      )}
      {incomplete && (
        <div className="import-pool-warning">
          <p>
            {t(
              "Die Erfassung ist nicht vollständig. Dichte oder unterbrochene Zeitfenster bleiben offen; der Import gilt nicht als abgeschlossen.",
            )}
          </p>
          {job.incomplete_windows.length > 0 && (
            <details>
              <summary>
                {job.incomplete_windows.length}{" "}
                {t("unvollständige Zeitfenster anzeigen")}
              </summary>
              <ul>
                {job.incomplete_windows.map((window, index) => (
                  <li key={`${window.start}-${window.end}-${index}`}>
                    {localTimestamp(window.start, job.timezone, locale)} {t("bis")}{" "}
                    {localTimestamp(window.end, job.timezone, locale)} (
                    {t("Endzeitpunkt ausgeschlossen")})
                  </li>
                ))}
              </ul>
            </details>
          )}
        </div>
      )}
      </details>
    </div>
  );
}
