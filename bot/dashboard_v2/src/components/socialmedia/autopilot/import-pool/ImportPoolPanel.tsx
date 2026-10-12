import { useEffect, useId, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Archive, Check, ExternalLink, RefreshCw, Save } from "lucide-react";
import { getBrowserTimezone } from "@/api/core";
import { useT } from "@/context/LanguageContext";
import { importPoolApi } from "./api";
import {
  activeImport,
  clipStatus,
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
import type {
  ImportJob,
  ImportPoolApi,
  ImportPoolPanelProps,
  PoolPreferences,
} from "./types";
import "./ImportPoolPanel.css";

const PAGE_SIZE = 50;
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
  const t = useT();
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
    queryFn: ({ signal }) => api.getLatestImport(owner, signal),
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
  const busy =
    saveMutation.isPending ||
    startMutation.isPending ||
    resumeMutation.isPending ||
    selectionMutation.isPending;
  const errors = [
    preferencesQuery.error,
    poolQuery.error,
    latestQuery.error,
    importQuery.error,
    saveMutation.error,
    startMutation.error,
    resumeMutation.error,
    selectionMutation.error,
  ].filter(Boolean);
  const blocked =
    errors.some(unauthorized) || preferencesQuery.isError || !saved;
  const page = poolQuery.data;
  const poolKnown =
    Boolean(saved && page && samePreferences(saved, page.preferences)) &&
    !poolQuery.isError &&
    !poolQuery.isFetching;
  const importKnown = latestQuery.isSuccess && !importQuery.isError;

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
    void preferencesQuery.refetch();
    void poolQuery.refetch();
    void latestQuery.refetch();
    if (activeImport(job)) void importQuery.refetch();
  }

  return (
    <section
      className="import-pool"
      aria-label={t("Historische Clips")}
      data-owner={owner}
      data-unsaved={dirty || busy ? "true" : undefined}
      data-saving={busy ? "true" : undefined}
    >
      <header className="import-pool-heading">
        <div>
          <h2>
            <Archive size={20} aria-hidden="true" />
            {t("Historische Clips")}
          </h2>
          <p>
            {t(
              "Deadlock-Clips erfassen und für die spätere Verarbeitung auswählen.",
            )}
          </p>
        </div>
        <button
          className="studio-button"
          type="button"
          onClick={reload}
          disabled={busy}
        >
          <RefreshCw size={16} aria-hidden="true" />
          {t("Erneut laden")}
        </button>
      </header>
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
              ? `${t(filterDescription(parsed.preferences.clip_date_filter))}. ${t("Kalendertage gelten in der gewählten Zeitzone. Zukünftige Clips werden nicht importiert.")}`
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
          <h3>{t("Clip-Pool")}</h3>
          <span className="import-pool-help">
            {page
              ? `${page.total} ${t("erfasste Clips")}`
              : t("Clip-Pool laden …")}
          </span>
        </div>
        {saved && (
          <p className="import-pool-help">
            {t(
              saved.clip_sort === "oldest_first"
                ? "Älteste Clips zuerst"
                : "Neueste Clips zuerst",
            )}
            .{" "}
            {t(
              "Sortiert nach ursprünglichem Clipdatum, bei gleichem Datum nach Clip-ID. Der Verlauf bleibt sichtbar.",
            )}{" "}
            {saved.timezone}
          </p>
        )}
        {poolQuery.isFetching && (
          <p role="status">{t("Clip-Pool wird aktualisiert …")}</p>
        )}
        {page && page.clips.length === 0 && (
          <p>
            {t(
              "In diesem Clip-Pool sind noch keine Clips erfasst. Mit einem historischen Import kannst du verfügbare Deadlock-Clips hinzufügen.",
            )}
          </p>
        )}
        {page && (
          <ul className="import-pool-clips">
            {page.clips.map((clip) => {
              const metadata = sourceMetadata(clip);
              return (
                <li
                  key={clip.clip_id}
                  data-clip-id={clip.clip_id}
                  className="import-pool-clip"
                >
                  <div>
                    <h4>{metadata.title}</h4>
                    <p className="import-pool-help">
                      {localTimestamp(
                        clip.source_created_at,
                        page.preferences.timezone,
                      )}
                      {metadata.duration !== null
                        ? ` · ${metadata.duration} s`
                        : ""}
                    </p>
                    <p className="import-pool-identity">
                      {t("Clip-ID")}: {clip.clip_id}
                    </p>
                    <p className="import-pool-clip-status">
                      <Check size={14} aria-hidden="true" />
                      {t(clipStatus(clip))}
                    </p>
                    {clip.first_selected_at && (
                      <p className="import-pool-help">
                        {t("Erstmals ausgewählt")}:{" "}
                        {localTimestamp(
                          clip.first_selected_at,
                          page.preferences.timezone,
                        )}
                      </p>
                    )}
                    {clip.processed_at && (
                      <p className="import-pool-help">
                        {t("Verarbeitet am")}:{" "}
                        {localTimestamp(
                          clip.processed_at,
                          page.preferences.timezone,
                        )}
                      </p>
                    )}
                    {clip.published_at && (
                      <p className="import-pool-help">
                        {t("Veröffentlicht am")}:{" "}
                        {localTimestamp(
                          clip.published_at,
                          page.preferences.timezone,
                        )}
                      </p>
                    )}
                    {!clip.eligible &&
                      (clip.selected_at ||
                        clip.processed_at ||
                        clip.published_at ||
                        clip.legacy_reserved) && (
                        <p className="import-pool-help">
                          {t("Außerhalb des gespeicherten Zeitraums")}
                        </p>
                      )}
                    {clip.clip_db_id === null && (
                      <p className="import-pool-help">
                        {t(
                          "Verlauf erhalten. Eine Quelldatei für die Verarbeitung ist nicht nachgewiesen.",
                        )}
                      </p>
                    )}
                  </div>
                  <div className="import-pool-clip-actions">
                    {metadata.url && (
                      <a
                        className="studio-button"
                        href={metadata.url}
                        target="_blank"
                        rel="noopener noreferrer"
                      >
                        <ExternalLink size={14} aria-hidden="true" />
                        {t("Auf Twitch ansehen")}
                      </a>
                    )}
                    <label>
                      <input
                        type="checkbox"
                        checked={clip.selected_at !== null}
                        disabled={
                          busy ||
                          blocked ||
                          !poolKnown ||
                          !selectionAllowed(clip)
                        }
                        aria-label={`${t("Clip auswählen")}: ${metadata.title}`}
                        onChange={(event) => {
                          setFeedback("");
                          selectionMutation.mutate({
                            clipId: clip.clip_id,
                            selected: event.target.checked,
                            after: onChanged,
                          });
                        }}
                      />
                      {t(clip.selected_at ? "Ausgewählt" : "Auswählen")}
                    </label>
                  </div>
                </li>
              );
            })}
          </ul>
        )}
        {page && page.total > page.limit && (
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
              {t("Seite")} {Math.floor(offset / PAGE_SIZE) + 1} /{" "}
              {Math.max(1, Math.ceil(page.total / PAGE_SIZE))}
            </span>
            <button
              type="button"
              className="studio-button"
              disabled={
                busy ||
                poolQuery.isFetching ||
                offset + page.limit >= page.total
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
