import { useEffect, useId, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Archive, ExternalLink, Film, RefreshCw, Save } from "lucide-react";
import { getBrowserTimezone } from "@/api/core";
import { useLanguage, useT } from "@/context/LanguageContext";
import { importPoolApi } from "./api";
import {
  activeImport,
  clipDuration,
  clipStatus,
  clipStatusTone,
  clipThumbnail,
  filterDescription,
  localTimestamp,
  parsePreferencesDraft,
  poolError,
  preferencesDraft,
  samePreferences,
  selectionAllowed,
  sourceMetadata,
  unauthorized,
  type PreferencesDraft,
} from "./presentation";
import { ImportProgress } from "./ImportProgress";
import { StudioSectionHeading } from '../../StudioSectionHeading';
import type {
  ImportJob,
  ImportPoolApi,
  ImportPoolPanelProps,
  PoolPreferences,
} from "./types";
import "./ImportPoolPanel.css";

const PAGE_SIZE = 24;
const POLL_INTERVAL = 5000;

type Completion = { after?: () => void };

export function ImportPoolPanel({
  twitchUserId,
  api = importPoolApi,
  onChanged,
}: ImportPoolPanelProps) {
  const t = useT();
  if (!twitchUserId || !/^\d+$/.test(twitchUserId)) {
    return (
      <section className="import-pool">
        <p>
          {t(
            "Bitte einen Twitch-Kanal auswählen, um historische Clips zu importieren.",
          )}
        </p>
      </section>
    );
  }
  return (
    <OwnerImportPool
      key={twitchUserId}
      owner={twitchUserId}
      api={api}
      onChanged={onChanged}
    />
  );
}

function OwnerImportPool({
  owner,
  api,
  onChanged,
}: {
  owner: string;
  api: ImportPoolApi;
  onChanged?: () => void;
}) {
  const { t, locale } = useLanguage();
  const queryClient = useQueryClient();
  const id = useId();
  const mounted = useRef(false);
  const [draft, setDraft] = useState<PreferencesDraft | null>(null);
  const [offset, setOffset] = useState(0);
  const [feedback, setFeedback] = useState("");
  const scope = ["social-media", "autopilot", "import-pool", owner];
  const preferencesKey = [...scope, "preferences"];
  const poolKey = [...scope, "pool"];
  const latestKey = [...scope, "latest-import"];

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const preferencesQuery = useQuery({
    queryKey: preferencesKey,
    queryFn: ({ signal }) => api.getPreferences(owner, signal),
    retry: false,
    refetchOnWindowFocus: false,
  });
  const saved = preferencesQuery.data;
  const current = draft ?? (saved ? preferencesDraft(saved) : null);
  const parsed = current ? parsePreferencesDraft(current) : null;
  const dirty = Boolean(
    draft &&
    saved &&
    (!parsed?.preferences || !samePreferences(parsed.preferences, saved)),
  );
  const poolQuery = useQuery({
    queryKey: [...poolKey, saved, offset],
    queryFn: ({ signal }) => api.getPool(owner, PAGE_SIZE, offset, signal),
    enabled: Boolean(saved) && !preferencesQuery.isError,
    retry: false,
    refetchOnWindowFocus: false,
  });
  const latestQuery = useQuery({
    queryKey: latestKey,
    queryFn: async ({ signal }) => {
      const latest = await api.getLatestImport(owner, signal);
      if (latest.import) {
        const key = [...scope, "import", latest.import.id];
        if (!unauthorized(queryClient.getQueryState(key)?.error)) {
          queryClient.setQueryData(key, latest.import);
        }
      }
      return latest;
    },
    retry: false,
    refetchOnWindowFocus: false,
  });
  const latestJob = latestQuery.data?.import;
  const importQuery = useQuery({
    queryKey: [...scope, "import", latestJob?.id],
    queryFn: ({ signal }) => api.getImport(owner, latestJob!.id, signal),
    enabled: activeImport(latestJob),
    initialData: latestJob ?? undefined,
    refetchInterval: (query) =>
      !query.state.error && activeImport(query.state.data)
        ? POLL_INTERVAL
        : false,
    refetchOnWindowFocus: false,
    retry: false,
  });
  const job = activeImport(latestJob)
    ? (importQuery.data ?? latestJob)
    : latestJob;
  const importCount = job?.persisted_count;
  const importStatus = job?.status;

  useEffect(() => {
    if (importCount !== undefined) {
      void queryClient.invalidateQueries({
        queryKey: ["social-media", "autopilot", "import-pool", owner, "pool"],
      });
    }
  }, [importCount, importStatus, owner, queryClient]);

  function changed(after: Completion["after"]) {
    void queryClient.invalidateQueries({ queryKey: ["social-media", "clips"] });
    after?.();
  }

  function acceptJob(next: ImportJob, after: Completion["after"]) {
    queryClient.setQueryData(latestKey, { import: next });
    queryClient.setQueryData([...scope, "import", next.id], next);
    void queryClient.invalidateQueries({ queryKey: poolKey });
    changed(after);
    if (mounted.current)
      setFeedback(
        "Importauftrag übernommen. Die Erfassung startet keine Videoproduktion.",
      );
  }

  const saveMutation = useMutation({
    mutationFn: ({
      preferences,
    }: Completion & { preferences: PoolPreferences }) =>
      api.savePreferences(owner, preferences),
    onMutate: async () => {
      await queryClient.cancelQueries({ queryKey: preferencesKey });
    },
    onSuccess: (preferences, { after }) => {
      queryClient.setQueryData(preferencesKey, preferences);
      void queryClient.invalidateQueries({ queryKey: poolKey });
      changed(after);
      if (mounted.current) {
        setDraft(null);
        setOffset(0);
        setFeedback(
          "Zeitraum und Reihenfolge gespeichert. Bestehende Aufträge bleiben erhalten.",
        );
      }
    },
  });
  async function prepareImportMutation() {
    await queryClient.cancelQueries({ queryKey: latestKey });
    await queryClient.cancelQueries({ queryKey: [...scope, "import"] });
  }

  const startMutation = useMutation({
    mutationFn: (_completion: Completion) => api.startImport(owner),
    onMutate: prepareImportMutation,
    onSuccess: (next, { after }) => acceptJob(next, after),
  });
  const resumeMutation = useMutation({
    mutationFn: ({ jobId }: Completion & { jobId: number }) =>
      api.resumeImport(owner, jobId),
    onMutate: prepareImportMutation,
    onSuccess: (next, { after }) => acceptJob(next, after),
  });
  const selectionMutation = useMutation({
    mutationFn: ({
      clipId,
      selected,
    }: Completion & { clipId: string; selected: boolean }) =>
      api.setSelection(owner, clipId, selected),
    onSuccess: (_selection, { after }) => {
      void queryClient.invalidateQueries({ queryKey: poolKey });
      changed(after);
      if (mounted.current) setFeedback("Clip-Auswahl gespeichert.");
    },
    onError: () => {
      void queryClient.invalidateQueries({ queryKey: poolKey });
    },
  });
  const bulkMutation = useMutation({
    mutationFn: async ({
      clipIds,
      selected,
    }: Completion & { clipIds: string[]; selected: boolean }) => {
      for (const clipId of clipIds) {
        await api.setSelection(owner, clipId, selected);
      }
    },
    onSuccess: (_result, { after, selected }) => {
      void queryClient.invalidateQueries({ queryKey: poolKey });
      changed(after);
      if (mounted.current)
        setFeedback(
          selected
            ? "Clips auf dieser Seite ausgewählt."
            : "Auswahl auf dieser Seite aufgehoben.",
        );
    },
    onError: () => {
      void queryClient.invalidateQueries({ queryKey: poolKey });
    },
  });
  const busy =
    saveMutation.isPending ||
    startMutation.isPending ||
    resumeMutation.isPending ||
    selectionMutation.isPending ||
    bulkMutation.isPending;
  const errors = [
    preferencesQuery.error,
    poolQuery.error,
    latestQuery.error,
    importQuery.error,
    saveMutation.error,
    startMutation.error,
    resumeMutation.error,
    selectionMutation.error,
    bulkMutation.error,
  ].filter(Boolean);
  const blocked =
    errors.some(unauthorized) || preferencesQuery.isError || !saved;
  const page = poolQuery.data;
  const poolKnown =
    Boolean(saved && page && samePreferences(saved, page.preferences)) &&
    !poolQuery.isError &&
    !poolQuery.isFetching;
  const importKnown = latestQuery.isSuccess && !importQuery.isError;
  const pageClips = page?.clips ?? [];
  const selectableOnPage = pageClips.filter(
    (clip) => clip.selected_at === null && selectionAllowed(clip),
  );
  const selectedOnPage = pageClips.filter(
    (clip) => clip.selected_at !== null && selectionAllowed(clip),
  );
  const selectionLocked = busy || blocked || !poolKnown;

  function bulkSelect(clipIds: string[], selected: boolean) {
    if (selectionLocked || clipIds.length === 0) return;
    setFeedback("");
    bulkMutation.mutate({ clipIds, selected, after: onChanged });
  }

  useEffect(() => {
    if (!dirty && !busy) return;
    const preventUnload = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", preventUnload);
    return () => window.removeEventListener("beforeunload", preventUnload);
  }, [dirty, busy]);

  function edit(change: Partial<PreferencesDraft>) {
    if (!current || busy || blocked) return;
    setDraft({ ...current, ...change });
    setFeedback("");
    saveMutation.reset();
  }

  function reload() {
    saveMutation.reset();
    startMutation.reset();
    resumeMutation.reset();
    selectionMutation.reset();
    bulkMutation.reset();
    void preferencesQuery.refetch();
    void poolQuery.refetch();
    void latestQuery.refetch();
    if (activeImport(job) || importQuery.isError) void importQuery.refetch();
  }

  return (
    <section
      className="import-pool"
      aria-label={t("Historische Clips")}
      data-owner={owner}
      data-unsaved={dirty || busy ? "true" : undefined}
      data-saving={busy ? "true" : undefined}
    >
      <StudioSectionHeading
        title={t("Historische Clips")}
        description={t("Deadlock-Clips erfassen. Markierte Clips werden in deiner gewählten Reihenfolge zuerst verarbeitet, danach die übrigen passenden Clips.")}
        icon={Archive}
        actions={<button className="studio-button" type="button" onClick={reload} disabled={busy}>
          <RefreshCw size={16} aria-hidden="true" />{t("Erneut laden")}
        </button>}
      />
      {errors.length > 0 && (
        <div role="alert" className="import-pool-error">
          {[...new Set(errors.map(poolError))].map((text) => (
            <p key={text}>{t(text)}</p>
          ))}
        </div>
      )}
      <p className="import-pool-feedback" role="status">
        {feedback ? t(feedback) : ""}
      </p>
      {!current && !preferencesQuery.isError && (
        <p role="status">{t("Gespeicherten Zeitraum laden …")}</p>
      )}
      {current && (
        <form
          className="import-pool-preferences"
          onSubmit={(event) => {
            event.preventDefault();
            if (busy || blocked || !dirty || !parsed?.preferences) return;
            setFeedback("");
            saveMutation.mutate({
              preferences: parsed.preferences,
              after: onChanged,
            });
          }}
        >
          <h3>{t("Zeitraum und Reihenfolge")}</h3>
          <fieldset
            disabled={busy || blocked}
            aria-describedby={`${id}-date-help`}
          >
            <div className="import-pool-fields">
              <label htmlFor={`${id}-kind`}>
                {t("Zeitraum")}
                <select
                  id={`${id}-kind`}
                  className="studio-select"
                  value={current.kind}
                  onChange={(event) =>
                    edit({
                      kind: event.target.value as PreferencesDraft["kind"],
                    })
                  }
                >
                  <option value="all">{t("Alle historischen Clips")}</option>
                  <option value="year">{t("Ein Jahr")}</option>
                  <option value="quarter">{t("Ein Quartal")}</option>
                  <option value="range">{t("Eigener Zeitraum")}</option>
                  <option value="before">{t("Vor einem Datum")}</option>
                  <option value="after">{t("Nach einem Datum")}</option>
                </select>
              </label>
              {(current.kind === "year" || current.kind === "quarter") && (
                <label htmlFor={`${id}-year`}>
                  {t("Jahr")}
                  <input
                    id={`${id}-year`}
                    className="studio-input"
                    type="number"
                    step="1"
                    required
                    value={current.year}
                    onChange={(event) => edit({ year: event.target.value })}
                  />
                </label>
              )}
              {current.kind === "quarter" && (
                <label htmlFor={`${id}-quarter`}>
                  {t("Quartal")}
                  <select
                    id={`${id}-quarter`}
                    className="studio-select"
                    value={current.quarter}
                    onChange={(event) => edit({ quarter: event.target.value })}
                  >
                    {[1, 2, 3, 4].map((quarter) => (
                      <option key={quarter} value={quarter}>
                        {t("Quartal")} {quarter}
                      </option>
                    ))}
                  </select>
                </label>
              )}
              {current.kind === "range" && (
                <>
                  <label htmlFor={`${id}-start`}>
                    {t("Beginn")}
                    <input
                      id={`${id}-start`}
                      className="studio-input"
                      type="date"
                      required
                      value={current.start}
                      onChange={(event) => edit({ start: event.target.value })}
                    />
                  </label>
                  <label htmlFor={`${id}-end`}>
                    {t("Ende")}
                    <input
                      id={`${id}-end`}
                      className="studio-input"
                      type="date"
                      required
                      value={current.end}
                      onChange={(event) => edit({ end: event.target.value })}
                    />
                  </label>
                </>
              )}
              {(current.kind === "before" || current.kind === "after") && (
                <label htmlFor={`${id}-date`}>
                  {t("Datum")}
                  <input
                    id={`${id}-date`}
                    className="studio-input"
                    type="date"
                    required
                    value={current.date}
                    onChange={(event) => edit({ date: event.target.value })}
                  />
                </label>
              )}
              <label htmlFor={`${id}-sort`}>
                {t("Reihenfolge")}
                <select
                  id={`${id}-sort`}
                  className="studio-select"
                  value={current.clipSort}
                  onChange={(event) =>
                    edit({
                      clipSort: event.target
                        .value as PreferencesDraft["clipSort"],
                    })
                  }
                >
                  <option value="oldest_first">
                    {t("Älteste Clips zuerst")}
                  </option>
                  <option value="newest_first">
                    {t("Neueste Clips zuerst")}
                  </option>
                </select>
              </label>
              <label htmlFor={`${id}-timezone`}>
                {t("Zeitzone")}
                <input
                  id={`${id}-timezone`}
                  className="studio-input"
                  required
                  spellCheck={false}
                  value={current.timezone}
                  onChange={(event) => edit({ timezone: event.target.value })}
                />
              </label>
            </div>
            <button
              className="import-pool-text-button"
              type="button"
              onClick={() => edit({ timezone: getBrowserTimezone() })}
            >
              {t("Browser-Zeitzone übernehmen")}
            </button>
          </fieldset>
          <p id={`${id}-date-help`} className="import-pool-help">
            {parsed?.preferences
              ? `${filterDescription(parsed.preferences.clip_date_filter, t)}. ${t("Kalendertage gelten in der gewählten Zeitzone. Zukünftige Clips werden nicht importiert.")}`
              : t(parsed?.error ?? "")}
          </p>
          <p className="import-pool-help">
            {t(
              "Änderungen gelten für neue Auswahlen und Importe, nicht für vorhandene Aufträge oder Videos. Der Import lädt keine Videodateien und startet keine Verarbeitung oder Veröffentlichung.",
            )}
          </p>
          <div className="import-pool-save">
            <span>
              {t(
                saveMutation.isPending
                  ? "Wird gespeichert …"
                  : dirty
                    ? "Ungespeicherte Änderungen"
                    : "Gespeicherter Stand",
              )}
            </span>
            <div>
              <button
                type="button"
                className="studio-button"
                disabled={!dirty || busy}
                onClick={() => {
                  setDraft(null);
                  saveMutation.reset();
                  setFeedback("");
                }}
              >
                {t("Verwerfen")}
              </button>
              <button
                type="submit"
                className="studio-primary"
                disabled={busy || blocked || !dirty || !parsed?.preferences}
              >
                <Save size={16} aria-hidden="true" />
                {t("Zeitraum speichern")}
              </button>
            </div>
          </div>
        </form>
      )}
      <section
        className="import-pool-import"
        aria-label={t("Historischer Import")}
      >
        <div className="import-pool-heading">
          <div>
            <h3>{t("Historischer Import")}</h3>
            <p>
              {t(
                "Verwendet den gespeicherten Zeitraum. Ein laufender Import behält seinen ursprünglichen Zeitraum.",
              )}
            </p>
          </div>
          <button
            type="button"
            className="studio-button"
            disabled={
              busy || blocked || dirty || !importKnown || activeImport(job)
            }
            onClick={() => {
              setFeedback("");
              startMutation.mutate({ after: onChanged });
            }}
          >
            {t(
              startMutation.isPending
                ? "Import wird gestartet …"
                : "Import starten",
            )}
          </button>
        </div>
        {dirty && (
          <p className="import-pool-help">
            {t("Änderungen vor dem Import speichern oder verwerfen.")}
          </p>
        )}
        {latestQuery.isPending ? (
          <p role="status">{t("Importstatus laden …")}</p>
        ) : job ? (
          <ImportProgress job={job} />
        ) : (
          latestQuery.isSuccess && (
            <p className="import-pool-help">
              {t("Noch kein historischer Import vorhanden.")}
            </p>
          )
        )}
        {job && ["error", "incomplete"].includes(job.status) && (
          <button
            type="button"
            className="studio-button"
            disabled={busy || blocked || !importKnown}
            onClick={() => {
              setFeedback("");
              resumeMutation.mutate({ jobId: job.id, after: onChanged });
            }}
          >
            {t(
              resumeMutation.isPending
                ? "Import wird fortgesetzt …"
                : "Import fortsetzen",
            )}
          </button>
        )}
      </section>
      <section className="import-pool-library" aria-label={t("Clip-Pool")}>
        <div className="import-pool-heading">
          <div>
            <h3>{t("Clip-Pool")}</h3>
            {saved && (
              <p>
                {t(
                  saved.clip_sort === "oldest_first"
                    ? "Älteste Clips zuerst"
                    : "Neueste Clips zuerst",
                )}
                {" · "}
                {page
                  ? `${page.total.toLocaleString(locale)} ${t("erfasste Clips")}`
                  : t(
                      poolQuery.isError
                        ? "Clip-Pool nicht geladen"
                        : "Clip-Pool laden …",
                    )}
              </p>
            )}
          </div>
          {page && page.clips.length > 0 && (
            <div className="import-pool-bulk">
              <button
                type="button"
                className="studio-button"
                disabled={selectionLocked || selectableOnPage.length === 0}
                onClick={() =>
                  bulkSelect(
                    selectableOnPage.map((clip) => clip.clip_id),
                    true,
                  )
                }
              >
                {t(
                  bulkMutation.isPending
                    ? "Auswahl wird gespeichert …"
                    : "Alle auf dieser Seite auswählen",
                )}
              </button>
              <button
                type="button"
                className="studio-button"
                disabled={selectionLocked || selectedOnPage.length === 0}
                onClick={() =>
                  bulkSelect(
                    selectedOnPage.map((clip) => clip.clip_id),
                    false,
                  )
                }
              >
                {t("Auswahl aufheben")}
              </button>
            </div>
          )}
        </div>
        {poolQuery.isFetching && (
          <p role="status" className="import-pool-help">
            {t("Clip-Pool wird aktualisiert …")}
          </p>
        )}
        {page && page.clips.length === 0 && (
          <p className="import-pool-empty">
            {t(
              "In diesem Clip-Pool sind noch keine Clips erfasst. Mit einem historischen Import kannst du verfügbare Deadlock-Clips hinzufügen.",
            )}
          </p>
        )}
        {page && page.clips.length > 0 && (
          <ul className="import-pool-clips">
            {page.clips.map((clip) => {
              const metadata = sourceMetadata(clip);
              const thumbnail = clipThumbnail(clip);
              const selected = clip.selected_at !== null;
              const timezone = page.preferences.timezone;
              const notes = [
                clip.first_selected_at &&
                  `${t("Erstmals ausgewählt")}: ${localTimestamp(clip.first_selected_at, timezone, locale)}`,
                clip.processed_at &&
                  `${t("Verarbeitet am")}: ${localTimestamp(clip.processed_at, timezone, locale)}`,
                clip.published_at &&
                  `${t("Veröffentlicht am")}: ${localTimestamp(clip.published_at, timezone, locale)}`,
                !clip.eligible &&
                  (clip.selected_at ||
                    clip.processed_at ||
                    clip.published_at ||
                    clip.legacy_reserved) &&
                  t("Außerhalb des gespeicherten Zeitraums"),
                clip.clip_db_id === null &&
                  t(
                    "Verlauf erhalten. Eine Quelldatei für die Verarbeitung ist nicht nachgewiesen.",
                  ),
              ].filter((note): note is string => Boolean(note));
              return (
                <li
                  key={clip.clip_id}
                  data-clip-id={clip.clip_id}
                  data-selected={selected ? "true" : undefined}
                  className="import-pool-clip"
                >
                  <div className="import-pool-thumb">
                    {thumbnail ? (
                      <img src={thumbnail} alt="" loading="lazy" />
                    ) : (
                      <Film size={28} aria-hidden="true" />
                    )}
                    <label className="import-pool-pick">
                      <input
                        type="checkbox"
                        checked={selected}
                        disabled={selectionLocked || !selectionAllowed(clip)}
                        aria-label={`${t("Clip bevorzugen")}: ${metadata.title}`}
                        title={t(selected ? "Bevorzugt" : "Zuerst verarbeiten")}
                        onChange={(event) => {
                          setFeedback("");
                          selectionMutation.mutate({
                            clipId: clip.clip_id,
                            selected: event.target.checked,
                            after: onChanged,
                          });
                        }}
                      />
                    </label>
                    {metadata.duration !== null && (
                      <span className="import-pool-duration">
                        {clipDuration(metadata.duration)}
                      </span>
                    )}
                  </div>
                  <div className="import-pool-clip-body">
                    <h4 title={`${t("Clip-ID")}: ${clip.clip_id}`}>
                      {metadata.title}
                    </h4>
                    <p className="import-pool-help">
                      {localTimestamp(clip.source_created_at, timezone, locale)}
                      {typeof clip.view_count === "number" &&
                        ` · ${clip.view_count.toLocaleString(locale)} ${t("Aufrufe")}`}
                    </p>
                    <div className="import-pool-clip-foot">
                      <span
                        className="import-pool-chip"
                        data-tone={clipStatusTone(clip)}
                      >
                        {t(clipStatus(clip))}
                      </span>
                      {metadata.url && (
                        <a
                          className="import-pool-link"
                          href={metadata.url}
                          target="_blank"
                          rel="noopener noreferrer"
                          aria-label={`${t("Auf Twitch ansehen")}: ${metadata.title}`}
                          title={t("Auf Twitch ansehen")}
                        >
                          <ExternalLink size={15} aria-hidden="true" />
                        </a>
                      )}
                    </div>
                    {notes.length > 0 && (
                      <ul className="import-pool-notes">
                        {notes.map((note) => (
                          <li key={note}>{note}</li>
                        ))}
                      </ul>
                    )}
                  </div>
                </li>
              );
            })}
          </ul>
        )}
        {(offset > 0 || (page && page.total > page.limit)) && (
          <nav
            className="import-pool-pagination"
            aria-label={t("Clip-Pool-Seiten")}
          >
            <button
              type="button"
              className="studio-button"
              disabled={busy || poolQuery.isFetching || offset === 0}
              onClick={() => setOffset(Math.max(0, offset - PAGE_SIZE))}
            >
              {t("Zurück")}
            </button>
            <span>
              {t("Seite")} {Math.floor(offset / PAGE_SIZE) + 1}
              {page && ` / ${Math.max(1, Math.ceil(page.total / PAGE_SIZE))}`}
            </span>
            <button
              type="button"
              className="studio-button"
              disabled={
                busy || !poolKnown || !page || offset + page.limit >= page.total
              }
              onClick={() => setOffset(offset + PAGE_SIZE)}
            >
              {t("Weiter")}
            </button>
          </nav>
        )}
      </section>
    </section>
  );
}
