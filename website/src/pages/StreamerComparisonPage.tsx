import {
  startTransition,
  useDeferredValue,
  useEffect,
  useState,
} from "react";
import {
  Activity,
  ArrowUpRight,
  BarChart3,
  Clock3,
  ExternalLink,
  Radio,
  Search,
  ShieldCheck,
  Sparkles,
  TrendingUp,
  UsersRound,
  Waves,
} from "lucide-react";
import { PublicInfoFooter } from "@/components/layout/PublicInfoFooter";
import { PublicInfoHeader } from "@/components/layout/PublicInfoHeader";

type Days = 7 | 30 | 90;
type SortKey =
  | "viewerHours"
  | "averageViewers"
  | "streamHours"
  | "growth"
  | "raidImpact";

interface ComparisonPeriod {
  days: Days;
  from: string;
  to: string;
  timezone: string;
  trendDays: number;
}

interface Methodology {
  cohort: string;
  minimumHoursForRanking: number;
  raidMeasurement: string;
  privacy: string;
  caveat: string;
}

interface NetworkSummary {
  streamerCount: number;
  qualifiedStreamerCount: number;
  streamHours: number;
  viewerHours: number;
  confirmedRaids: number;
  viewersForwarded: number;
  measuredRaids: number;
  averageRaidUplift5m: number | null;
  averageRaidUplift30m: number | null;
  positiveRaidShare30m: number | null;
}

interface StreamerRanks {
  streamHours: number | null;
  averageViewers: number | null;
  viewerHours: number | null;
  momentum: number | null;
  raidImpact: number | null;
}

interface NextStep {
  code: string;
  title: string;
  reason: string;
}

interface StreamerRow {
  login: string;
  displayName: string;
  twitchUrl: string;
  sampleQualified: boolean;
  trendQualified: boolean;
  sessions: number;
  streamHours: number;
  averageViewers: number;
  peakViewers: number | null;
  viewerHours: number;
  recentHours: number;
  recentAverageViewers: number | null;
  previousHours: number;
  previousAverageViewers: number | null;
  viewerGrowthPct: number | null;
  confirmedRaids: number;
  raidViewersReceived: number;
  measuredRaids: number;
  raidDataQualified: boolean;
  raidUplift5m: number | null;
  raidUplift30m: number | null;
  positiveRaidShare30m: number | null;
  ranks: StreamerRanks;
  nextStep: NextStep;
}

interface ComparisonResponse {
  generatedAt: string;
  period: ComparisonPeriod;
  methodology: Methodology;
  network: NetworkSummary;
  streamers: StreamerRow[];
}

const ENDPOINT = "/twitch/api/v2/public/streamer-comparison";
const DAY_OPTIONS: Days[] = [7, 30, 90];

const SORT_OPTIONS: Array<{ value: SortKey; label: string }> = [
  { value: "viewerHours", label: "Viewer-Stunden" },
  { value: "averageViewers", label: "Ø Zuschauer" },
  { value: "growth", label: "Momentum" },
  { value: "raidImpact", label: "Raid-Effekt" },
  { value: "streamHours", label: "Streamzeit" },
];

const SORT_RANKS: Record<SortKey, keyof StreamerRanks> = {
  viewerHours: "viewerHours",
  averageViewers: "averageViewers",
  streamHours: "streamHours",
  growth: "momentum",
  raidImpact: "raidImpact",
};

function number(value: number, digits = 0) {
  return new Intl.NumberFormat("de-DE", {
    maximumFractionDigits: digits,
    minimumFractionDigits: digits,
  }).format(value);
}

function signed(value: number | null, suffix = "") {
  if (value === null) return "–";
  const prefix = value > 0 ? "+" : "";
  return `${prefix}${number(value, 1)}${suffix}`;
}

function rank(value: number | null) {
  return value === null ? "nicht gerankt" : `#${value}`;
}

function sortValue(streamer: StreamerRow, key: SortKey) {
  if (key === "growth") return streamer.viewerGrowthPct ?? Number.NEGATIVE_INFINITY;
  if (key === "raidImpact") return streamer.raidUplift30m ?? Number.NEGATIVE_INFINITY;
  return streamer[key];
}

function compareStreamers(left: StreamerRow, right: StreamerRow, key: SortKey) {
  const rankKey = SORT_RANKS[key];
  const leftRanked = left.ranks[rankKey] !== null;
  const rightRanked = right.ranks[rankKey] !== null;
  if (leftRanked !== rightRanked) return leftRanked ? -1 : 1;
  return sortValue(right, key) - sortValue(left, key) || left.login.localeCompare(right.login);
}

function QualityBadge({ qualified, label }: { qualified: boolean; label: string }) {
  return (
    <span
      className={`inline-flex items-center gap-1 rounded-full border px-2.5 py-1 text-[11px] font-bold uppercase tracking-[0.12em] ${
        qualified
          ? "border-success/35 bg-success-soft text-success"
          : "border-border bg-white/[0.025] text-text-secondary"
      }`}
    >
      <span className={`h-1.5 w-1.5 rounded-full ${qualified ? "bg-success" : "bg-text-secondary"}`} />
      {label}
    </span>
  );
}

function SummaryCard({
  icon,
  label,
  value,
  detail,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  detail: string;
}) {
  return (
    <article className="panel-card rounded-2xl p-5">
      <div className="flex items-start justify-between gap-4">
        <div>
          <p className="text-xs font-bold uppercase tracking-[0.16em] text-text-secondary">{label}</p>
          <p className="mt-3 font-display text-3xl font-bold text-text-primary">{value}</p>
        </div>
        <div className="icon-tile rounded-xl p-2.5">{icon}</div>
      </div>
      <p className="mt-3 text-sm leading-relaxed text-text-secondary">{detail}</p>
    </article>
  );
}

function LoadingState() {
  return (
    <div className="panel-card rounded-3xl p-10 text-center" role="status">
      <Radio className="mx-auto animate-pulse text-primary" size={28} />
      <p className="mt-4 font-display text-lg font-semibold">Netzwerkdaten werden abgeglichen</p>
      <p className="mt-2 text-sm text-text-secondary">Sessions, Raids und Vergleichsfenster laufen durch dieselbe Messung.</p>
    </div>
  );
}

function Spotlight({ streamer, trendDays }: { streamer: StreamerRow; trendDays: number }) {
  const rankCards = [
    { label: "Viewer-Stunden", value: rank(streamer.ranks.viewerHours) },
    { label: "Ø Zuschauer", value: rank(streamer.ranks.averageViewers) },
    { label: "Momentum", value: rank(streamer.ranks.momentum) },
  ];

  return (
    <section className="panel-card relative rounded-3xl border-primary/35 p-6 md:p-8" aria-label={`${streamer.displayName} im Detail`}>
      <div className="pointer-events-none absolute inset-y-0 right-0 w-1/2 bg-[radial-gradient(circle_at_right,rgba(85,151,143,0.12),transparent_65%)]" />
      <div className="relative grid gap-8 lg:grid-cols-[1.1fr_0.9fr]">
        <div>
          <div className="flex flex-wrap items-center gap-3">
            <p className="text-xs font-bold uppercase tracking-[0.18em] text-primary">Kanal-Fokus</p>
            <QualityBadge qualified={streamer.sampleQualified} label={streamer.sampleQualified ? "belastbar" : "kleine Stichprobe"} />
            <QualityBadge qualified={streamer.raidDataQualified} label={streamer.raidDataQualified ? "Raid-Basis" : "Raid-Testphase"} />
          </div>
          <div className="mt-4 flex flex-wrap items-end gap-4">
            <h2 className="font-display text-3xl font-bold text-text-primary md:text-4xl">{streamer.displayName}</h2>
            <a
              href={streamer.twitchUrl}
              target="_blank"
              rel="noreferrer"
              className="mb-1 inline-flex items-center gap-1.5 text-sm font-semibold text-accent transition-colors hover:text-accent-hover"
            >
              Twitch öffnen <ExternalLink size={14} />
            </a>
          </div>

          <div className="mt-6 grid grid-cols-3 gap-2 sm:gap-3">
            {rankCards.map((card) => (
              <div key={card.label} className="rounded-xl border border-border bg-black/20 p-3 sm:p-4">
                <p className="text-[10px] font-bold uppercase tracking-[0.12em] text-text-secondary sm:text-xs">{card.label}</p>
                <p className="mt-2 font-display text-xl font-bold text-primary sm:text-2xl">{card.value}</p>
              </div>
            ))}
          </div>

          <dl className="mt-6 grid grid-cols-2 gap-x-5 gap-y-4 text-sm sm:grid-cols-4">
            <div>
              <dt className="text-text-secondary">Zeitraum-Schnitt</dt>
              <dd className="mt-1 text-lg font-bold">{number(streamer.averageViewers, 2)}</dd>
            </div>
            <div>
              <dt className="text-text-secondary">Letzte {trendDays} Tage</dt>
              <dd className="mt-1 text-lg font-bold">{streamer.recentAverageViewers === null ? "–" : number(streamer.recentAverageViewers, 2)}</dd>
            </div>
            <div>
              <dt className="text-text-secondary">Raid-Lift +30 Min.</dt>
              <dd className={`mt-1 text-lg font-bold ${(streamer.raidUplift30m ?? 0) > 0 ? "text-success" : "text-text-primary"}`}>
                {signed(streamer.raidUplift30m)}
              </dd>
            </div>
            <div>
              <dt className="text-text-secondary">Bestätigte Raids</dt>
              <dd className="mt-1 text-lg font-bold">{streamer.confirmedRaids}</dd>
            </div>
          </dl>
        </div>

        <aside className="relative rounded-2xl border border-accent/30 bg-accent-soft p-5 sm:p-6">
          <div className="flex items-center gap-2 text-accent">
            <Sparkles size={18} />
            <p className="text-xs font-bold uppercase tracking-[0.16em]">Nächster sinnvoller Test</p>
          </div>
          <h3 className="mt-4 font-display text-xl font-bold text-text-primary">{streamer.nextStep.title}</h3>
          <p className="mt-3 text-sm leading-7 text-text-secondary">{streamer.nextStep.reason}</p>
          <div className="mt-5 border-t border-accent/20 pt-4 text-xs leading-6 text-text-secondary">
            Die Empfehlung ist eine sichtbare Heuristik aus genau den Zahlen dieser Karte, keine versteckte KI-Bewertung.
          </div>
        </aside>
      </div>
    </section>
  );
}

function MobileStreamerCard({ streamer, onSelect }: { streamer: StreamerRow; onSelect: () => void }) {
  return (
    <article className="panel-card rounded-2xl p-5 lg:hidden">
      <div className="flex items-start justify-between gap-4">
        <button type="button" onClick={onSelect} className="border-0 bg-transparent p-0 text-left text-text-primary">
          <span className="font-display text-lg font-bold">{streamer.displayName}</span>
          <span className="mt-1 block text-xs text-text-secondary">{number(streamer.streamHours, 1)} h · {streamer.sessions} Streams</span>
        </button>
        <QualityBadge qualified={streamer.sampleQualified} label={streamer.sampleQualified ? "gerankt" : "Daten"} />
      </div>
      <div className="mt-5 grid grid-cols-3 gap-2 text-center">
        <div className="rounded-xl bg-black/20 p-3">
          <p className="text-[10px] uppercase tracking-wide text-text-secondary">Ø Viewer</p>
          <p className="mt-1 font-bold">{number(streamer.averageViewers, 2)}</p>
        </div>
        <div className="rounded-xl bg-black/20 p-3">
          <p className="text-[10px] uppercase tracking-wide text-text-secondary">Momentum</p>
          <p className={`mt-1 font-bold ${(streamer.viewerGrowthPct ?? 0) > 0 ? "text-success" : ""}`}>{signed(streamer.viewerGrowthPct, "%")}</p>
        </div>
        <div className="rounded-xl bg-black/20 p-3">
          <p className="text-[10px] uppercase tracking-wide text-text-secondary">Raid +30</p>
          <p className={`mt-1 font-bold ${(streamer.raidUplift30m ?? 0) > 0 ? "text-success" : ""}`}>{signed(streamer.raidUplift30m)}</p>
        </div>
      </div>
      <button type="button" onClick={onSelect} className="mt-4 inline-flex w-full items-center justify-center gap-2 rounded-xl border border-border bg-white/[0.03] px-4 py-2.5 text-sm font-semibold text-text-primary">
        Im Detail vergleichen <ArrowUpRight size={15} />
      </button>
    </article>
  );
}

export function StreamerComparisonPage() {
  const [days, setDays] = useState<Days>(() => {
    if (typeof window === "undefined") return 30;
    const requested = Number(new URLSearchParams(window.location.search).get("days"));
    return DAY_OPTIONS.includes(requested as Days) ? requested as Days : 30;
  });
  const [data, setData] = useState<ComparisonResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [sort, setSort] = useState<SortKey>("viewerHours");
  const [selectedLogin, setSelectedLogin] = useState<string | null>(() => {
    if (typeof window === "undefined") return null;
    return new URLSearchParams(window.location.search).get("streamer");
  });
  const deferredSearch = useDeferredValue(search.trim().toLowerCase());

  useEffect(() => {
    const controller = new AbortController();
    setLoading(true);
    setError(null);

    fetch(`${ENDPOINT}?days=${days}`, { signal: controller.signal })
      .then(async (response) => {
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        return (await response.json()) as ComparisonResponse;
      })
      .then((payload) => {
        const url = new URL(window.location.href);
        const requestedLogin = url.searchParams.get("streamer");
        const nextLogin = requestedLogin && payload.streamers.some((streamer) => streamer.login === requestedLogin)
          ? requestedLogin
          : payload.streamers[0]?.login ?? null;
        if (nextLogin) url.searchParams.set("streamer", nextLogin);
        else url.searchParams.delete("streamer");
        window.history.replaceState({}, "", url);

        startTransition(() => {
          setData(payload);
          setSelectedLogin(nextLogin);
          setLoading(false);
        });
      })
      .catch((reason: unknown) => {
        if (reason instanceof DOMException && reason.name === "AbortError") return;
        setError("Die Vergleichsdaten sind gerade nicht erreichbar.");
        setData(null);
        setLoading(false);
      });

    return () => controller.abort();
  }, [days]);

  const visibleStreamers = data
    ? data.streamers
        .filter((streamer) => !deferredSearch || streamer.displayName.toLowerCase().includes(deferredSearch) || streamer.login.includes(deferredSearch))
        .sort((left, right) => compareStreamers(left, right, sort))
    : [];
  const selected = data?.streamers.find((streamer) => streamer.login === selectedLogin) ?? visibleStreamers[0] ?? null;
  const maxViewerHours = Math.max(...visibleStreamers.map((streamer) => streamer.viewerHours), 1);
  const signalStreamers = data
    ? data.streamers.filter((streamer) => streamer.sampleQualified).sort((left, right) => right.averageViewers - left.averageViewers).slice(0, 12)
    : [];
  const maxSignal = Math.max(...signalStreamers.map((streamer) => streamer.averageViewers), 1);

  function selectStreamer(login: string) {
    setSelectedLogin(login);
    const url = new URL(window.location.href);
    url.searchParams.set("streamer", login);
    window.history.replaceState({}, "", url);
    document.getElementById("kanal-fokus")?.scrollIntoView({ behavior: "smooth", block: "start" });
  }

  function changeDays(nextDays: Days) {
    setDays(nextDays);
    const url = new URL(window.location.href);
    url.searchParams.set("days", String(nextDays));
    window.history.replaceState({}, "", url);
  }

  return (
    <>
      <PublicInfoHeader
        navLinks={[
          { label: "Überblick", href: "#ueberblick" },
          { label: "Vergleich", href: "#vergleich" },
          { label: "Methodik", href: "#methodik" },
        ]}
        primaryAction={{ label: "Streamer werden", href: "/twitch/onboarding" }}
        secondaryAction={{ label: "Dashboard", href: "/analyse" }}
      />

      <main className="overflow-hidden pt-16">
        <section id="ueberblick" className="relative border-b border-border px-5 pb-14 pt-16 sm:px-6 md:pb-20 md:pt-24">
          <div className="pointer-events-none absolute -left-36 top-0 h-96 w-96 rounded-full bg-primary/10 blur-[120px]" />
          <div className="relative mx-auto max-w-7xl">
            <div className="grid items-end gap-10 lg:grid-cols-[1.15fr_0.85fr]">
              <div>
                <div className="flex items-center gap-3 text-primary">
                  <Waves size={19} />
                  <p className="text-xs font-bold uppercase tracking-[0.22em]">DDL Netzwerk-Puls</p>
                </div>
                <h1 className="mt-5 max-w-4xl font-display text-4xl font-bold leading-[1.05] text-text-primary sm:text-5xl md:text-6xl">
                  Nicht wer am lautesten ist.
                  <span className="mt-2 block bg-gradient-to-r from-primary-hover via-primary to-accent bg-clip-text text-transparent">Sondern was wirklich wirkt.</span>
                </h1>
                <p className="mt-6 max-w-2xl text-base leading-8 text-text-secondary md:text-lg">
                  Alle aktiven Partner werden mit derselben Messung verglichen. So sehen Streamer, wo Momentum entsteht, welche Raids hängen bleiben und welcher nächste Test sinnvoll ist.
                </p>
              </div>

              <div className="panel-card rounded-2xl p-5 sm:p-6">
                <div className="flex items-center justify-between gap-4">
                  <div>
                    <p className="text-xs font-bold uppercase tracking-[0.16em] text-text-secondary">Zeitraum</p>
                    <p className="mt-1 text-sm text-text-primary">Rollierend bis jetzt</p>
                  </div>
                  <Clock3 className="text-primary" size={20} />
                </div>
                <div className="mt-5 grid grid-cols-3 gap-2">
                  {DAY_OPTIONS.map((option) => (
                    <button
                      key={option}
                      type="button"
                      onClick={() => changeDays(option)}
                      aria-pressed={days === option}
                      className={`rounded-xl border px-3 py-3 text-sm font-bold transition-colors ${
                        days === option
                          ? "border-primary bg-primary-soft text-primary-hover"
                          : "border-border bg-black/20 text-text-secondary hover:border-border-hover hover:text-text-primary"
                      }`}
                    >
                      {option} Tage
                    </button>
                  ))}
                </div>
                <p className="mt-4 text-xs leading-5 text-text-secondary">Rankings erscheinen erst ab der zum Zeitraum passenden Mindestmenge. Kleine Kanäle bleiben sichtbar, werden aber nicht unfair einsortiert.</p>
              </div>
            </div>

            {data ? (
              <div className="mt-10 flex h-28 items-end gap-1.5 overflow-hidden rounded-2xl border border-border bg-black/25 px-4 pb-4 pt-6" aria-label="Zuschauerschnitt der zwölf stärksten Signale">
                {signalStreamers.map((streamer) => (
                  <button
                    key={streamer.login}
                    type="button"
                    onClick={() => selectStreamer(streamer.login)}
                    className="group relative flex h-full min-w-0 flex-1 items-end border-0 bg-transparent p-0"
                    title={`${streamer.displayName}: Ø ${number(streamer.averageViewers, 2)}`}
                  >
                    <span
                      className="block w-full rounded-t-sm bg-gradient-to-t from-primary/35 to-primary transition-colors group-hover:from-accent/50 group-hover:to-accent"
                      style={{ height: `${Math.max(10, (streamer.averageViewers / maxSignal) * 100)}%` }}
                    />
                  </button>
                ))}
              </div>
            ) : null}
          </div>
        </section>

        <section className="px-5 py-12 sm:px-6 md:py-16">
          <div className="mx-auto max-w-7xl">
            {loading ? <LoadingState /> : null}
            {error ? (
              <div className="panel-card rounded-2xl border-danger/40 p-8 text-center">
                <p className="font-display text-xl font-bold">{error}</p>
                <button type="button" onClick={() => window.location.reload()} className="mt-5 rounded-xl border border-border px-4 py-2 text-sm font-semibold">Neu laden</button>
              </div>
            ) : null}

            {data && !loading ? (
              <>
                <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
                  <SummaryCard icon={<UsersRound size={20} />} label="Aktive Streamer" value={String(data.network.streamerCount)} detail={`${data.network.qualifiedStreamerCount} erfüllen die Mindeststichprobe für Rankings.`} />
                  <SummaryCard icon={<Activity size={20} />} label="Viewer-Stunden" value={number(data.network.viewerHours, 0)} detail={`${number(data.network.streamHours, 0)} gestreamte Stunden im Netzwerk.`} />
                  <SummaryCard icon={<Radio size={20} />} label="Bestätigte Raids" value={String(data.network.confirmedRaids)} detail={`${data.network.viewersForwarded} Zuschauer wurden innerhalb des Netzwerks weitergereicht.`} />
                  <SummaryCard icon={<TrendingUp size={20} />} label="Raid-Lift nach 30 Min." value={signed(data.network.averageRaidUplift30m)} detail={`${data.network.positiveRaidShare30m === null ? "Noch keine Basis" : `${number(data.network.positiveRaidShare30m, 0)} %`} der messbaren Raids lagen nach 30 Minuten über ihrer Vorphase.`} />
                </div>

                <div id="kanal-fokus" className="scroll-mt-24 pt-8">
                  {selected ? <Spotlight streamer={selected} trendDays={data.period.trendDays} /> : null}
                </div>

                <section id="vergleich" className="scroll-mt-24 pt-14 md:pt-20">
                  <div className="flex flex-col gap-5 lg:flex-row lg:items-end lg:justify-between">
                    <div>
                      <p className="text-xs font-bold uppercase tracking-[0.18em] text-primary">Alle Kanäle</p>
                      <h2 className="mt-3 font-display text-3xl font-bold md:text-4xl">Vergleichen, ohne Äpfel mit Birnen zu mischen.</h2>
                      <p className="mt-3 max-w-2xl text-sm leading-7 text-text-secondary">Nicht gerankte Zeilen bleiben sichtbar. Damit wird wenig Sendezeit nicht versehentlich als starke oder schwache Leistung verkauft.</p>
                    </div>
                    <div className="grid gap-3 sm:grid-cols-2">
                      <label className="relative block">
                        <Search className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-text-secondary" size={17} />
                        <input
                          value={search}
                          onChange={(event) => setSearch(event.target.value)}
                          aria-label="Streamer suchen"
                          placeholder="Streamer suchen"
                          className="h-11 w-full rounded-xl border border-border bg-black/25 pl-10 pr-4 text-sm text-text-primary outline-none transition-colors placeholder:text-text-secondary focus:border-primary sm:w-56"
                        />
                      </label>
                      <select
                        value={sort}
                        onChange={(event) => setSort(event.target.value as SortKey)}
                        className="h-11 rounded-xl border border-border bg-[#151513] px-4 text-sm font-semibold text-text-primary outline-none focus:border-primary"
                        aria-label="Sortierung"
                      >
                        {SORT_OPTIONS.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
                      </select>
                    </div>
                  </div>

                  <div className="mt-7 space-y-3 lg:hidden">
                    {visibleStreamers.map((streamer) => <MobileStreamerCard key={streamer.login} streamer={streamer} onSelect={() => selectStreamer(streamer.login)} />)}
                  </div>

                  <div className="panel-card mt-7 hidden overflow-hidden rounded-2xl lg:block">
                    <div className="overflow-x-auto">
                      <table className="w-full border-collapse text-left text-sm">
                        <thead className="border-b border-border bg-black/25 text-[11px] uppercase tracking-[0.12em] text-text-secondary">
                          <tr>
                            <th className="px-5 py-4 font-bold">Streamer</th>
                            <th className="px-4 py-4 font-bold">Ø Zuschauer</th>
                            <th className="min-w-44 px-4 py-4 font-bold">Viewer-Stunden</th>
                            <th className="px-4 py-4 font-bold">Momentum</th>
                            <th className="px-4 py-4 font-bold">Raids</th>
                            <th className="px-4 py-4 font-bold">Raid +30 Min.</th>
                            <th className="px-5 py-4 font-bold">Nächster Test</th>
                          </tr>
                        </thead>
                        <tbody>
                          {visibleStreamers.map((streamer) => (
                            <tr key={streamer.login} className={`border-b border-border/60 transition-colors last:border-0 ${selected?.login === streamer.login ? "bg-primary-soft" : "hover:bg-white/[0.025]"}`}>
                              <td className="px-5 py-4">
                                <button type="button" onClick={() => selectStreamer(streamer.login)} className="border-0 bg-transparent p-0 text-left text-text-primary">
                                  <span className="block font-display font-bold">{streamer.displayName}</span>
                                  <span className="mt-1 block text-xs text-text-secondary">{number(streamer.streamHours, 1)} h · {streamer.sessions} Streams</span>
                                </button>
                              </td>
                              <td className="px-4 py-4">
                                <span className="font-bold">{number(streamer.averageViewers, 2)}</span>
                                <span className="mt-1 block text-xs text-text-secondary">{rank(streamer.ranks.averageViewers)}</span>
                              </td>
                              <td className="px-4 py-4">
                                <div className="flex items-center justify-between gap-3">
                                  <span className="font-bold">{number(streamer.viewerHours, 1)}</span>
                                  <span className="text-xs text-text-secondary">{rank(streamer.ranks.viewerHours)}</span>
                                </div>
                                <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-black/35">
                                  <div className="h-full rounded-full bg-gradient-to-r from-primary to-accent" style={{ width: `${Math.max(2, (streamer.viewerHours / maxViewerHours) * 100)}%` }} />
                                </div>
                              </td>
                              <td className="px-4 py-4">
                                <span className={`font-bold ${(streamer.viewerGrowthPct ?? 0) > 0 ? "text-success" : (streamer.viewerGrowthPct ?? 0) < 0 ? "text-danger" : ""}`}>{signed(streamer.viewerGrowthPct, "%")}</span>
                                <span className="mt-1 block text-xs text-text-secondary">{rank(streamer.ranks.momentum)}</span>
                              </td>
                              <td className="px-4 py-4">
                                <span className="font-bold">{streamer.confirmedRaids}</span>
                                <span className="mt-1 block text-xs text-text-secondary">{streamer.raidViewersReceived} Viewer</span>
                              </td>
                              <td className="px-4 py-4">
                                <span className={`font-bold ${(streamer.raidUplift30m ?? 0) > 0 ? "text-success" : ""}`}>{signed(streamer.raidUplift30m)}</span>
                                <span className="mt-1 block text-xs text-text-secondary">{streamer.measuredRaids} messbar</span>
                              </td>
                              <td className="max-w-72 px-5 py-4">
                                <button type="button" onClick={() => selectStreamer(streamer.login)} className="inline-flex items-center gap-2 border-0 bg-transparent p-0 text-left text-xs font-semibold leading-5 text-accent hover:text-accent-hover">
                                  {streamer.nextStep.title} <ArrowUpRight className="shrink-0" size={14} />
                                </button>
                              </td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  </div>

                  {visibleStreamers.length === 0 ? (
                    <div className="mt-7 rounded-2xl border border-border bg-black/20 p-8 text-center text-text-secondary">Kein aktiver Streamer passt zur Suche.</div>
                  ) : null}
                </section>

                <section id="methodik" className="scroll-mt-24 pt-16 md:pt-24">
                  <div className="grid gap-8 lg:grid-cols-[0.75fr_1.25fr]">
                    <div>
                      <div className="flex items-center gap-2 text-primary"><ShieldCheck size={19} /><span className="text-xs font-bold uppercase tracking-[0.18em]">Methodik & Datenqualität</span></div>
                      <h2 className="mt-4 font-display text-3xl font-bold">Die Regeln stehen neben den Zahlen.</h2>
                      <p className="mt-4 text-sm leading-7 text-text-secondary">Keine Geheimformel und kein Umsatz-Ranking. Jede Kennzahl lässt sich aus öffentlichen Stream-Verläufen und bestätigten Netzwerk-Raids erklären.</p>
                    </div>
                    <div className="grid gap-4 sm:grid-cols-2">
                      <article className="panel-card rounded-2xl p-5"><UsersRound className="text-primary" size={19} /><h3 className="mt-4 font-display font-bold">Kohorte</h3><p className="mt-2 text-sm leading-6 text-text-secondary">{data.methodology.cohort}. Rankings ab {number(data.methodology.minimumHoursForRanking, 0)} Stunden.</p></article>
                      <article className="panel-card rounded-2xl p-5"><BarChart3 className="text-primary" size={19} /><h3 className="mt-4 font-display font-bold">Raid-Messung</h3><p className="mt-2 text-sm leading-6 text-text-secondary">{data.methodology.raidMeasurement}</p></article>
                      <article className="panel-card rounded-2xl p-5"><ShieldCheck className="text-primary" size={19} /><h3 className="mt-4 font-display font-bold">Privatsphäre</h3><p className="mt-2 text-sm leading-6 text-text-secondary">{data.methodology.privacy}</p></article>
                      <article className="panel-card rounded-2xl p-5"><Activity className="text-primary" size={19} /><h3 className="mt-4 font-display font-bold">Wirkung, nicht Kausalität</h3><p className="mt-2 text-sm leading-6 text-text-secondary">{data.methodology.caveat}</p></article>
                    </div>
                  </div>
                  <div className="mt-8 flex flex-col gap-3 rounded-2xl border border-border bg-black/20 px-5 py-4 text-xs text-text-secondary sm:flex-row sm:items-center sm:justify-between">
                    <span>Zuletzt berechnet: {new Date(data.generatedAt).toLocaleString("de-DE", { timeZone: "Europe/Berlin" })}</span>
                    <span>Zeitzone: {data.period.timezone} · Zeitraum: {data.period.days} Tage</span>
                  </div>
                </section>
              </>
            ) : null}
          </div>
        </section>
      </main>

      <PublicInfoFooter />
    </>
  );
}
