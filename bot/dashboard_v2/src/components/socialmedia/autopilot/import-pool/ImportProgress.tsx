import { useLanguage, useT } from "@/context/LanguageContext";
import {
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
  return (
    <div className="import-pool-progress" data-import-status={job.status}>
      <p
        className={
          incomplete || job.status === "error"
            ? "import-pool-warning"
            : "import-pool-progress-title"
        }
        role="status"
      >
        {t(importStatusLabel(job))}
      </p>
      <p className="import-pool-help">
        {t("Festgelegter Zeitraum")}:{" "}
        {filterDescription(job.clip_date_filter, t)}. {job.timezone}
      </p>
      <p className="import-pool-help">
        {t("Erfassung von")} {localTimestamp(job.started_at, job.timezone, locale)}{" "}
        {t("bis")} {localTimestamp(job.ended_at, job.timezone, locale)} (
        {t("Endzeitpunkt ausgeschlossen")}).
      </p>
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
      {job.next_retry_at && (
        <p>
          {t("Nächster Versuch frühestens")}:{" "}
          {localTimestamp(job.next_retry_at, job.timezone, locale)}
        </p>
      )}
      {job.last_error && (
        <p className="import-pool-warning">{t(job.last_error)}</p>
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
    </div>
  );
}
