import { globalTimezoneUrl } from '@/utils/socialMediaChannel';
import { ImportPopover } from '../../ImportPopover';
import { useEffect, useId, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Archive, Save } from "lucide-react";

import { useLanguage, useT } from "@/context/LanguageContext";
import { importPoolApi } from "./api";
import { activeImport, filterDescription, parsePreferencesDraft, poolError, preferencesDraft, samePreferences, unauthorized, type PreferencesDraft } from "./presentation";
import { PipelineClips } from "../../PipelineClips";
import { loadSourceCollection, poolBackedSources } from "../../pipelineApi";
import { ImportProgress } from "./ImportProgress";
import type { ImportJob, ImportPoolApi, ImportPoolPanelProps, PoolPreferences } from "./types";
import type { QueueStage } from "../../queuePresentation";
import "./ImportPoolPanel.css";

const POLL_INTERVAL = 5000;

type Completion = { after?: () => void };

export function ImportPoolPanel({
  twitchUserId,
  api = importPoolApi,
  onChanged,
  reloadKey = 0,
  initialStage,
}: ImportPoolPanelProps) {
  const t = useT();
  if (!twitchUserId || !/^\d+$/.test(twitchUserId)) {
    return (
      <section className="import-pool studio-card">
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
      reloadKey={reloadKey}
      initialStage={initialStage}
    />
  );
}

function OwnerImportPool({
  owner,
  api,
  onChanged,
  reloadKey,
  initialStage,
}: {
  owner: string;
  api: ImportPoolApi;
  onChanged?: () => void;
  reloadKey: number;
  initialStage?: QueueStage;
}) {
  const { t, locale } = useLanguage();
  const queryClient = useQueryClient();
  const id = useId();
  const mounted = useRef(false);
  const [draft, setDraft] = useState<PreferencesDraft | null>(null);
  const [feedback, setFeedback] = useState("");
  const scope = ["social-media", "autopilot", "import-pool", owner];
  const preferencesKey = [...scope, "preferences"];
  const poolKey = [...scope, "sources"];
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
    queryKey: [...poolKey, saved],
    queryFn: ({ signal }) => loadSourceCollection(api.getSources ?? poolBackedSources(api), owner, signal),
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
        queryKey: ["social-media", "autopilot", "import-pool", owner, "sources"],
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

  function clearSettledFeedback() {
    for (const mutation of [saveMutation, startMutation, resumeMutation, selectionMutation, bulkMutation]) {
      if (!mutation.isPending) mutation.reset();
    }
  }

  function reload() {
    clearSettledFeedback();
    void preferencesQuery.refetch();
    void poolQuery.refetch();
    void latestQuery.refetch();
    if (activeImport(job) || importQuery.isError) void importQuery.refetch();
  }

  const clearRef = useRef(clearSettledFeedback);
  useEffect(() => { clearRef.current = clearSettledFeedback; });
  const seenReloadKey = useRef(reloadKey);
  useEffect(() => {
    if (seenReloadKey.current === reloadKey) return;
    seenReloadKey.current = reloadKey;
    clearRef.current();
  }, [reloadKey]);

  return (
    <section
      className="import-pool studio-card"
      aria-label={t("Clip-Vorrat")}
      data-owner={owner}
      data-unsaved={dirty || busy ? "true" : undefined}
      data-saving={busy ? "true" : undefined}
    >
      <div className="import-pool-toolbar">
        <div className="import-pool-toolbar-title">
          <h2><Archive size={18} aria-hidden="true" />{t("Clip-Vorrat (Pool)")}</h2>
          <p>
            {page
              ? t("{count} Clips erfasst", { count: page.total.toLocaleString(locale) })
              : t(poolQuery.isError ? "Clip-Pool nicht geladen" : "Clip-Pool laden …")}
            {saved && ` · ${t(saved.clip_sort === "oldest_first" ? "Älteste Clips zuerst" : "Neueste Clips zuerst")}`}
          </p>
        </div>
        <div className="import-pool-toolbar-actions">
          <details className="import-pool-history">
            <summary className="studio-button">
              {job ? t("Historischer Import ({count} erfasst)", { count: job.persisted_count.toLocaleString(locale) }) : t("Historischer Import")}
            </summary>
            <div className="import-pool-history-panel" aria-label={t("Historischer Import")} role="group">
              <p className="import-pool-help">
                {t(
                  "Verwendet den gespeicherten Zeitraum. Ein laufender Import behält seinen ursprünglichen Zeitraum.",
                )}
              </p>
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
            </div>
          </details>
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
          {current && (
            <ImportPopover><form
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
                </div>
                <a href={globalTimezoneUrl(owner)}>{t("Zeitzone in Verwaltung ändern")}</a>
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
            </form></ImportPopover>
          )}
        </div>
      </div>
      {errors.length > 0 && (
        <div role="alert" className="import-pool-error">
          {[...new Set(errors.map(poolError))].map((text) => (
            <p key={text}>{t(text)}</p>
          ))}
          <button className="studio-button" type="button" onClick={reload} disabled={busy}>
            {t("Erneut laden")}
          </button>
        </div>
      )}
      <p className="import-pool-feedback" role="status">
        {feedback ? t(feedback) : ""}
      </p>
      {!current && !preferencesQuery.isError && (
        <p role="status">{t("Gespeicherten Zeitraum laden …")}</p>
      )}
      <div className="import-pool-library">
        {poolQuery.isFetching && (
          <p role="status" className="import-pool-help">
            {t("Clip-Pool wird aktualisiert …")}
          </p>
        )}
        {page && (
          <PipelineClips
            page={page}
            owner={owner}
            initialStage={initialStage}
            selectionLocked={selectionLocked}
            onSelect={(clipId, selected) => {
              if (selectionLocked) return;
              setFeedback("");
              selectionMutation.mutate({ clipId, selected, after: onChanged });
            }}
            onBulkSelect={bulkSelect}
            bulkPending={bulkMutation.isPending}
            onArchived={() => {
              void queryClient.invalidateQueries({ queryKey: poolKey });
              void queryClient.invalidateQueries({ queryKey: ["social-posting-plan"] });
              onChanged?.();
            }}
          />
        )}
      </div>
    </section>
  );
}
