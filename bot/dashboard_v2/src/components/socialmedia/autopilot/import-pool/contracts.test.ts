import assert from "node:assert/strict";
import { test } from "node:test";
import type { ClipDateFilter, PoolPreferences } from "./types";

Object.defineProperty(globalThis, "window", {
  configurable: true,
  value: { location: new URL("https://fixture.invalid/twitch/social-media") },
});
const { ApiHttpError } = await import("@/api/core");
const { importPoolApi } = await import("./api");
const { createImportPoolFixture, createImportPoolFixtureJob } =
  await import("./fixtures");
const {
  activeImport,
  parsePreferencesDraft,
  preferencesDraft,
  samePreferences,
  selectionAllowed,
  sourceMetadata,
  filterDescription,
  localTimestamp,
} = await import("./presentation");
const { dictionaryFor, translate } = await import('@/i18n/dictionary');

test('filter descriptions translate stable keys with intact parameters for all six filters', () => {
  const filters: ClipDateFilter[] = [
    { kind: 'all' },
    { kind: 'year', year: 2025 },
    { kind: 'quarter', year: 2025, quarter: 4 },
    { kind: 'range', start: '2025-03-30', end: '2025-04-01' },
    { kind: 'before', date: '2025-03-30' },
    { kind: 'after', date: '2025-03-30' },
  ];
  for (const filter of filters) {
    let calls = 0;
    const en = filterDescription(filter, (key, params) => {
      calls += 1;
      assert.ok(Object.hasOwn(dictionaryFor('en'), key));
      for (const value of Object.values(params ?? {})) assert.ok(translate('en', key, params).includes(String(value)));
      return translate('en', key, params);
    });
    assert.equal(calls, 1);
    assert.notEqual(en, filterDescription(filter));
    assert.doesNotMatch(en, /\{\w+\}/);
    assert.equal(filterDescription(filter, (key, params) => translate('de', key, params)), filterDescription(filter));
  }
});

test('import timestamps use the display locale without changing the stored timezone', () => {
  const value = '2025-03-30T10:15:00Z';
  const timezone = 'Europe/Berlin';
  for (const locale of ['de-DE', 'en-US']) {
    assert.equal(localTimestamp(value, timezone, locale), new Intl.DateTimeFormat(locale, { dateStyle: 'medium', timeStyle: 'short', timeZone: timezone }).format(new Date(value)));
  }
  assert.notEqual(localTimestamp(value, timezone, 'en-US'), localTimestamp(value, timezone, 'de-DE'));
});

const preferences: PoolPreferences = {
  clip_date_filter: { kind: "all" },
  clip_sort: "oldest_first",
  timezone: "Europe/Berlin",
};

test("all six tagged filters round-trip without widening the saved contract", () => {
  const filters: ClipDateFilter[] = [
    { kind: "all" },
    { kind: "year", year: 2025 },
    { kind: "quarter", year: 2025, quarter: 4 },
    { kind: "range", start: "2025-03-30", end: "2025-03-30" },
    { kind: "before", date: "2025-03-30" },
    { kind: "after", date: "2025-03-30" },
  ];
  for (const clip_date_filter of filters) {
    const saved = { ...preferences, clip_date_filter };
    const parsed = parsePreferencesDraft(preferencesDraft(saved, 2025));
    assert.equal(parsed.error, null);
    assert.deepEqual(parsed.preferences, saved);
    assert.equal(samePreferences(parsed.preferences!, saved), true);
    assert.deepEqual(Object.keys(parsed.preferences!).sort(), [
      "clip_date_filter",
      "clip_sort",
      "timezone",
    ]);
  }
});

test("invalid dates, reversed ranges, fractional years and invalid zones cannot be submitted", () => {
  const draft = preferencesDraft(preferences, 2025);
  for (const change of [
    { kind: "range" as const, start: "2025-02-29", end: "2025-03-01" },
    { kind: "range" as const, start: "2025-04-01", end: "2025-03-31" },
    { kind: "before" as const, date: "" },
    { kind: "year" as const, year: "2025.5" },
    { kind: "quarter" as const, quarter: "0" },
    { timezone: "Mars/Unknown" },
    { timezone: "+02:00" },
  ]) {
    const parsed = parsePreferencesDraft({ ...draft, ...change });
    assert.equal(parsed.preferences, null);
    assert.equal(typeof parsed.error, "string");
  }
  assert.ok(
    parsePreferencesDraft({
      ...draft,
      kind: "range",
      start: "2024-02-29",
      end: "2024-02-29",
    }).preferences,
  );
});

test("saved timezone remains authoritative until explicitly edited", () => {
  const saved = { ...preferences, timezone: "America/New_York" };
  const draft = preferencesDraft(saved, 2025);
  assert.equal(draft.timezone, saved.timezone);
  assert.equal(
    samePreferences(parsePreferencesDraft(draft).preferences!, saved),
    true,
  );
  assert.equal(
    samePreferences(
      parsePreferencesDraft({ ...draft, timezone: "UTC" }).preferences!,
      saved,
    ),
    false,
  );
});

test("the real HTTP adapter scopes and escapes requests and sends only owned preferences", async (context) => {
  const calls: Array<{ path: string; init: RequestInit }> = [];
  context.mock.method(
    globalThis,
    "fetch",
    async (input: RequestInfo | URL, init: RequestInit = {}) => {
      calls.push({ path: String(input), init });
      return new Response(JSON.stringify(preferences), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    },
  );
  const controller = new AbortController();
  await importPoolApi.getPreferences("101", controller.signal);
  await importPoolApi.getPool("101", 50, 100, controller.signal);
  await importPoolApi.getLatestImport("101");
  await importPoolApi.getImport("101", 71);
  await importPoolApi.startImport("101");
  await importPoolApi.resumeImport("101", 71);
  await importPoolApi.setSelection("101", "Stable/id?x=#", true);
  await importPoolApi.savePreferences("101", {
    ...preferences,
    enabled: true,
  } as PoolPreferences);
  const prefix = "/twitch/social-media/api/autopilot";
  assert.deepEqual(
    calls.map((call) => call.path),
    [
      `${prefix}/preferences?twitch_user_id=101`,
      `${prefix}/pool?twitch_user_id=101&limit=50&offset=100`,
      `${prefix}/imports?twitch_user_id=101`,
      `${prefix}/imports/71?twitch_user_id=101`,
      `${prefix}/imports?twitch_user_id=101`,
      `${prefix}/imports/71/resume?twitch_user_id=101`,
      `${prefix}/pool/Stable%2Fid%3Fx%3D%23/selection?twitch_user_id=101`,
      `${prefix}/preferences?twitch_user_id=101`,
    ],
  );
  assert.deepEqual(
    calls.map((call) => call.init.method ?? "GET"),
    ["GET", "GET", "GET", "GET", "POST", "POST", "PUT", "PUT"],
  );
  assert.ok(calls.every((call) => call.init.credentials === "same-origin"));
  assert.equal(calls[0].init.signal, controller.signal);
  assert.equal(calls[4].init.body, undefined);
  assert.equal(calls[5].init.body, undefined);
  assert.deepEqual(JSON.parse(String(calls[6].init.body)), { selected: true });
  assert.deepEqual(JSON.parse(String(calls[7].init.body)), preferences);
  assert.throws(() => importPoolApi.getLatestImport("fixture_login"));
  assert.equal(calls.length, 8);
});

test("HTTP conflicts and unauthorized responses reject through the existing error type", async (context) => {
  for (const status of [400, 401, 403, 404, 409, 415, 422, 500]) {
    context.mock.method(
      globalThis,
      "fetch",
      async () =>
        new Response(
          JSON.stringify({
            error: "pool_state_conflict",
            message: "Synthetischer Fehler",
          }),
          { status },
        ),
    );
    await assert.rejects(
      importPoolApi.startImport("101"),
      (error: unknown) =>
        error instanceof ApiHttpError && error.status === status,
    );
    context.mock.restoreAll();
  }
});

test("only active import states poll and incomplete archives remain terminal", () => {
  assert.equal(activeImport(null), false);
  for (const status of [
    "queued",
    "running",
    "retry",
    "error",
    "incomplete",
    "completed",
  ] as const) {
    assert.equal(
      activeImport(createImportPoolFixtureJob({ status })),
      ["queued", "running", "retry"].includes(status),
    );
  }
});

test("fixture chronology has stable ascending identity ties in both date directions", async () => {
  const fixture = createImportPoolFixture();
  const oldest = await fixture.api.getPool("101", 50, 0);
  assert.deepEqual(
    oldest.clips.map((clip) => clip.clip_id),
    [
      "FixtureOldest",
      "FixturePublished",
      "FixtureTieA",
      "FixtureTieB",
      "FixtureSelected",
      "FixtureReserved",
    ],
  );
  await fixture.api.savePreferences("101", {
    ...preferences,
    clip_sort: "newest_first",
  });
  const newest = await fixture.api.getPool("101", 50, 0);
  assert.deepEqual(
    newest.clips.map((clip) => clip.clip_id),
    [
      "FixtureReserved",
      "FixtureSelected",
      "FixtureTieA",
      "FixtureTieB",
      "FixturePublished",
      "FixtureOldest",
    ],
  );
  assert.equal(
    newest.clips.find((clip) => clip.clip_id === "FixturePublished")
      ?.clip_db_id,
    null,
  );
  assert.equal((await fixture.api.getPool("202", 50, 0)).total, 0);
});

test("fixture writes preserve owner, frozen import and first-selection history", async () => {
  const fixture = createImportPoolFixture();
  const release = fixture.holdNext("savePreferences");
  const write = fixture.api.savePreferences("101", {
    ...preferences,
    clip_date_filter: { kind: "year", year: 2025 },
  });
  await fixture.api.getPreferences("202");
  release();
  await write;
  assert.deepEqual(fixture.snapshot("202").preferences, preferences);
  const started = await fixture.api.startImport("101");
  assert.deepEqual(await fixture.api.startImport("101"), started);
  await fixture.api.savePreferences("101", preferences);
  assert.deepEqual(fixture.snapshot().job?.clip_date_filter, {
    kind: "year",
    year: 2025,
  });
  await fixture.api.setSelection("101", "FixtureOldest", true);
  await fixture.api.setSelection("101", "FixtureOldest", false);
  const clip = fixture
    .snapshot()
    .clips.find((value) => value.clip_id === "FixtureOldest")!;
  assert.equal(clip.selected_at, null);
  assert.ok(clip.first_selected_at);
  fixture.failNext("setSelection", 409);
  await assert.rejects(
    fixture.api.setSelection("101", "FixtureOldest", true),
    ApiHttpError,
  );
  assert.equal(
    fixture.snapshot().clips.find((value) => value.clip_id === "FixtureOldest")
      ?.selected_at,
    null,
  );
});

test("protected history cannot be selected and source metadata is accessed defensively", async () => {
  const fixture = createImportPoolFixture();
  const page = await fixture.api.getPool("101", 50, 0);
  for (const clip of page.clips.filter(
    (value) => value.published_at || value.legacy_reserved,
  ))
    assert.equal(selectionAllowed(clip), false);
  const clip = page.clips[0];
  assert.deepEqual(sourceMetadata({ ...clip, source_metadata: null }), {
    title: clip.clip_id,
    url: null,
    duration: null,
  });
  assert.deepEqual(
    sourceMetadata({
      ...clip,
      source_metadata: {
        clip_url: "javascript:alert(1)",
        duration_seconds: -1,
      },
    }),
    { title: clip.clip_id, url: null, duration: null },
  );
});
