import { useCallback, useEffect, useState, type FormEvent, type ReactNode } from "react";
import {
  AlertCircle,
  ArrowRightLeft,
  CheckCircle,
  Clock,
  Download,
  FileText,
  LayoutDashboard,
  LogIn,
  Receipt,
  Settings,
  ShieldCheck,
  Users,
  XCircle,
} from "lucide-react";

type View = "login" | "dashboard";
type Tab = "overview" | "streamers" | "commissions" | "gutschriften" | "settings";
type UstStatus = "unknown" | "kleinunternehmer" | "regelbesteuert";

interface GutschriftReadiness {
  can_generate: boolean;
  blockers: string[];
  warnings: string[];
  missing_fields: string[];
  ust_status: UstStatus;
}

interface AffiliateProfile {
  twitch_login: string;
  display_name: string;
  email: string;
  full_name: string;
  address_line1: string;
  address_city: string;
  address_zip: string;
  address_country: string;
  tax_id: string;
  vat_id: string;
  ust_status: UstStatus;
  stripe_connect_status: string;
  stripe_account_id: string;
  is_active: boolean;
  created_at: string;
  updated_at: string;
  profile_updated_at?: string | null;
  gutschrift_readiness: GutschriftReadiness;
}

interface Claim {
  claimed_streamer_login?: string;
  streamer_login?: string;
  claimed_at: string;
  total_commission_cents: number;
  commission_count: number;
}

interface Commission {
  id: number;
  streamer_login: string;
  brutto_cents: number;
  commission_cents: number;
  currency: string;
  status: string;
  created_at: string;
}

interface Gutschrift {
  id: number;
  period_label: string;
  gutschrift_number: string;
  status: string;
  net_amount_cents: number;
  vat_amount_cents: number;
  gross_amount_cents: number;
  commission_count: number;
  generated_at: string | null;
  emailed_at: string | null;
  note_text: string;
  last_error: string;
  download_path: string | null;
  has_pdf: boolean;
}

interface SettingsFormState {
  full_name: string;
  email: string;
  address_line1: string;
  address_city: string;
  address_zip: string;
  address_country: string;
  tax_id: string;
  vat_id: string;
  ust_status: UstStatus;
}

const EMPTY_READINESS: GutschriftReadiness = {
  can_generate: false,
  blockers: [],
  warnings: [],
  missing_fields: [],
  ust_status: "unknown",
};

const COUNTRIES = [
  { value: "DE", label: "Deutschland" },
  { value: "AT", label: "Oesterreich" },
  { value: "CH", label: "Schweiz" },
  { value: "NL", label: "Niederlande" },
  { value: "BE", label: "Belgien" },
  { value: "LU", label: "Luxemburg" },
  { value: "FR", label: "Frankreich" },
  { value: "IT", label: "Italien" },
  { value: "ES", label: "Spanien" },
  { value: "PL", label: "Polen" },
];

const TABS: { id: Tab; label: string; icon: typeof LayoutDashboard }[] = [
  { id: "overview", label: "Uebersicht", icon: LayoutDashboard },
  { id: "streamers", label: "Streamer", icon: Users },
  { id: "commissions", label: "Provisionen", icon: Receipt },
  { id: "gutschriften", label: "Gutschriften", icon: FileText },
  { id: "settings", label: "Einstellungen", icon: Settings },
];

function formatCents(cents: number, currency = "EUR"): string {
  return new Intl.NumberFormat("de-DE", {
    style: "currency",
    currency: currency.toUpperCase(),
  }).format((cents || 0) / 100);
}

function formatDate(value: string | null | undefined): string {
  if (!value) return "—";
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) return "—";
  return parsed.toLocaleDateString("de-DE");
}

function formatDateTime(value: string | null | undefined): string {
  if (!value) return "—";
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) return "—";
  return parsed.toLocaleString("de-DE", {
    dateStyle: "medium",
    timeStyle: "short",
  });
}

function readJsonSafe<T>(response: Response): Promise<T> {
  return response.json() as Promise<T>;
}

function emptySettingsForm(): SettingsFormState {
  return {
    full_name: "",
    email: "",
    address_line1: "",
    address_city: "",
    address_zip: "",
    address_country: "DE",
    tax_id: "",
    vat_id: "",
    ust_status: "unknown",
  };
}

function normalizeProfileToForm(profile: AffiliateProfile | null): SettingsFormState {
  if (!profile) {
    return emptySettingsForm();
  }
  return {
    full_name: profile.full_name ?? "",
    email: profile.email ?? "",
    address_line1: profile.address_line1 ?? "",
    address_city: profile.address_city ?? "",
    address_zip: profile.address_zip ?? "",
    address_country: profile.address_country || "DE",
    tax_id: profile.tax_id ?? "",
    vat_id: profile.vat_id ?? "",
    ust_status: profile.ust_status ?? profile.gutschrift_readiness?.ust_status ?? "unknown",
  };
}

function profileErrorText(code: string): string {
  switch (code) {
    case "invalid_json":
      return "Die Daten konnten nicht gelesen werden.";
    case "invalid_payload":
      return "Das Profilformular ist ungueltig.";
    case "invalid_ust_status":
      return "Bitte waehle einen gueltigen USt-Status.";
    case "not_found":
      return "Affiliate-Profil nicht gefunden.";
    default:
      return code || "Profil konnte nicht gespeichert werden.";
  }
}

function claimErrorText(code: string): string {
  switch (code) {
    case "invalid_login":
      return "Bitte gib einen gueltigen Twitch-Login ein.";
    case "already_claimed":
      return "Dieser Streamer wurde bereits beansprucht.";
    case "streamer_already_registered":
      return "Dieser Streamer ist bereits als Partner registriert.";
    default:
      return code || "Fehler beim Beanspruchen.";
  }
}

function taxHint(status: UstStatus): string {
  switch (status) {
    case "kleinunternehmer":
      return "Gemäß § 19 UStG wird keine Umsatzsteuer berechnet.";
    case "regelbesteuert":
      return "19 % Umsatzsteuer werden auf den Nettobetrag ausgewiesen.";
    default:
      return "Gutschrift-Generierung blockiert, bis der USt-Status gepflegt ist.";
  }
}

function StatusBadge({ status }: { status: string }) {
  const styles: Record<string, string> = {
    transferred: "bg-emerald-500/20 text-emerald-400",
    emailed: "bg-emerald-500/20 text-emerald-400",
    generated: "bg-sky-500/20 text-sky-300",
    skipped: "bg-yellow-500/20 text-yellow-400",
    pending: "bg-gray-500/20 text-gray-300",
    blocked: "bg-amber-500/20 text-amber-300",
    email_failed: "bg-red-500/20 text-red-400",
    failed: "bg-red-500/20 text-red-400",
  };
  return (
    <span
      className={`inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-medium ${styles[status] ?? styles.pending}`}
    >
      {status}
    </span>
  );
}

export default function AffiliatePortal() {
  const [view, setView] = useState<View>("login");
  const [tab, setTab] = useState<Tab>("overview");
  const [profile, setProfile] = useState<AffiliateProfile | null>(null);
  const [claims, setClaims] = useState<Claim[]>([]);
  const [commissions, setCommissions] = useState<Commission[]>([]);
  const [gutschriften, setGutschriften] = useState<Gutschrift[]>([]);
  const [gutschriftReadiness, setGutschriftReadiness] =
    useState<GutschriftReadiness>(EMPTY_READINESS);
  const [settingsForm, setSettingsForm] = useState<SettingsFormState>(emptySettingsForm);
  const [claimInput, setClaimInput] = useState("");
  const [claimMsg, setClaimMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [profileMsg, setProfileMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [loading, setLoading] = useState(true);
  const [savingProfile, setSavingProfile] = useState(false);
  const [pageError, setPageError] = useState<string | null>(null);

  const fetchData = useCallback(async (options?: { background?: boolean }) => {
    const background = Boolean(options?.background);
    if (!background) {
      setLoading(true);
    }
    try {
      const profileRes = await fetch("/twitch/api/affiliate/me");
      if (profileRes.status === 401) {
        setView("login");
        setProfile(null);
        setClaims([]);
        setCommissions([]);
        setGutschriften([]);
        setPageError(null);
        return;
      }
      if (!profileRes.ok) {
        throw new Error("profile_fetch_failed");
      }

      const profileData = await readJsonSafe<AffiliateProfile>(profileRes);
      setProfile(profileData);
      setSettingsForm(normalizeProfileToForm(profileData));
      setGutschriftReadiness(profileData.gutschrift_readiness ?? EMPTY_READINESS);
      setView("dashboard");

      const [claimsRes, commissionsRes, gutschriftenRes] = await Promise.all([
        fetch("/twitch/api/affiliate/claims"),
        fetch("/twitch/api/affiliate/commissions"),
        fetch("/twitch/api/affiliate/gutschriften"),
      ]);

      if ([claimsRes, commissionsRes, gutschriftenRes].some((res) => res.status === 401)) {
        setView("login");
        setProfile(null);
        return;
      }

      const claimsData = claimsRes.ok
        ? await readJsonSafe<{ claims?: Claim[] }>(claimsRes)
        : { claims: [] };
      const commissionsData = commissionsRes.ok
        ? await readJsonSafe<{ commissions?: Commission[] }>(commissionsRes)
        : { commissions: [] };
      const gutschriftenData = gutschriftenRes.ok
        ? await readJsonSafe<{
            gutschriften?: Gutschrift[];
            readiness?: GutschriftReadiness;
          }>(gutschriftenRes)
        : { gutschriften: [], readiness: profileData.gutschrift_readiness };

      setClaims(claimsData.claims ?? []);
      setCommissions(commissionsData.commissions ?? []);
      setGutschriften(gutschriftenData.gutschriften ?? []);
      setGutschriftReadiness(gutschriftenData.readiness ?? profileData.gutschrift_readiness);
      setPageError(null);
    } catch {
      setPageError("Die Affiliate-Daten konnten nicht geladen werden.");
      setView("login");
    } finally {
      if (!background) {
        setLoading(false);
      }
    }
  }, []);

  useEffect(() => {
    void fetchData();
  }, [fetchData]);

  async function handleClaim() {
    const login = claimInput.trim();
    if (!login) return;
    setClaimMsg(null);
    try {
      const res = await fetch("/twitch/affiliate/claim", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ streamer_login: login }),
      });
      const data = await readJsonSafe<{ ok?: boolean; claimed?: string; error?: string }>(res);
      if (data.ok) {
        setClaimMsg({ ok: true, text: `${data.claimed ?? login} erfolgreich beansprucht.` });
        setClaimInput("");
        void fetchData({ background: true });
        return;
      }
      setClaimMsg({ ok: false, text: claimErrorText(data.error ?? "") });
    } catch {
      setClaimMsg({ ok: false, text: "Netzwerkfehler." });
    }
  }

  function updateForm<K extends keyof SettingsFormState>(field: K, value: SettingsFormState[K]) {
    setSettingsForm((current) => ({ ...current, [field]: value }));
  }

  async function handleProfileSave(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setSavingProfile(true);
    setProfileMsg(null);
    try {
      const payload = {
        ...settingsForm,
        address_country: settingsForm.address_country || "DE",
        vat_id: settingsForm.ust_status === "regelbesteuert" ? settingsForm.vat_id : "",
      };
      const res = await fetch("/twitch/api/affiliate/profile", {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(payload),
      });
      const data = await readJsonSafe<{
        ok?: boolean;
        error?: string;
        profile?: AffiliateProfile;
      }>(res);
      if (!res.ok || !data.ok || !data.profile) {
        setProfileMsg({ ok: false, text: profileErrorText(data.error ?? "") });
        return;
      }
      setProfile(data.profile);
      setSettingsForm(normalizeProfileToForm(data.profile));
      setGutschriftReadiness(data.profile.gutschrift_readiness ?? EMPTY_READINESS);
      setProfileMsg({ ok: true, text: "Profil erfolgreich gespeichert." });
      void fetchData({ background: true });
    } catch {
      setProfileMsg({ ok: false, text: "Profil konnte nicht gespeichert werden." });
    } finally {
      setSavingProfile(false);
    }
  }

  if (loading) {
    return (
      <div className="min-h-screen bg-[#07151d] flex items-center justify-center">
        <div className="w-8 h-8 border-2 border-[#06B6D4] border-t-transparent rounded-full animate-spin" />
      </div>
    );
  }

  if (view === "login") {
    return (
      <div className="min-h-screen bg-[#07151d] flex items-center justify-center p-6">
        <div className="panel-card rounded-2xl p-10 max-w-md w-full text-center">
          <div className="w-16 h-16 rounded-xl gradient-accent flex items-center justify-center mx-auto mb-6">
            <LogIn size={28} className="text-white" />
          </div>
          <h1 className="text-2xl font-bold text-[#e9f1f7] font-[Sora] mb-3">
            Affiliate-Portal
          </h1>
          <p className="text-[#9bb3c5] text-sm mb-8 leading-relaxed">
            Werde Vertriebler und verdiene 30% Provision auf jede Zahlung deiner
            geworbenen Streamer dauerhaft und ohne Limit.
          </p>
          <button
            onClick={() => {
              window.location.href = "/twitch/auth/affiliate/login";
            }}
            className="gradient-accent rounded-xl px-7 py-3.5 font-semibold text-white w-full transition-all duration-200 hover:brightness-110 hover:shadow-[0_0_24px_4px_rgba(255,122,24,0.3)]"
          >
            Mit Twitch anmelden
          </button>
          <div className="mt-4 rounded-xl border border-[rgba(255,122,24,0.25)] bg-[rgba(255,122,24,0.08)] px-4 py-3 text-left">
            <div className="flex items-start gap-3">
              <AlertCircle size={18} className="text-[#ffb067] mt-0.5 shrink-0" />
              <p className="text-xs leading-relaxed text-[#d7e5f0]">
                Steuerhinweis: Du bist selbst fuer die steuerliche Behandlung
                deiner Provisionen verantwortlich. Verbinde spaeter Stripe,
                damit Auszahlungen automatisch an dein Auszahlungs-Konto gehen
                koennen.
              </p>
            </div>
          </div>
        </div>
      </div>
    );
  }

  const readiness = gutschriftReadiness ?? profile?.gutschrift_readiness ?? EMPTY_READINESS;
  const totalCents = commissions.reduce((sum, item) => sum + item.commission_cents, 0);
  const transferredCents = commissions
    .filter((item) => item.status === "transferred")
    .reduce((sum, item) => sum + item.commission_cents, 0);
  const pendingCents = commissions
    .filter((item) => item.status === "pending")
    .reduce((sum, item) => sum + item.commission_cents, 0);
  const stripeConnected = profile?.stripe_connect_status === "connected";
  const needsNextSteps = claims.length === 0 || !stripeConnected || !readiness.can_generate;
  const openGutschriften = gutschriften.filter(
    (item) => item.status === "generated" || item.status === "email_failed",
  );
  const latestGutschrift = gutschriften[0] ?? null;
  const gutschriftGrossTotal = gutschriften.reduce(
    (sum, item) => sum + item.gross_amount_cents,
    0,
  );

  return (
    <div className="min-h-screen bg-[#07151d] text-[#e9f1f7] font-[Manrope]">
      <nav className="glass sticky top-0 z-50">
        <div className="max-w-7xl mx-auto px-6 flex items-center gap-1 h-14 overflow-x-auto">
          <span className="text-sm font-bold text-[#06B6D4] mr-4 shrink-0 font-[Sora]">
            Affiliate
          </span>
          {TABS.map((entry) => {
            const Icon = entry.icon;
            const active = tab === entry.id;
            return (
              <button
                key={entry.id}
                onClick={() => setTab(entry.id)}
                className={`flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-colors shrink-0 ${
                  active
                    ? "bg-[#102635] text-[#06B6D4]"
                    : "text-[#9bb3c5] hover:text-[#e9f1f7] hover:bg-[#102635]/50"
                }`}
              >
                <Icon size={16} />
                {entry.label}
              </button>
            );
          })}
        </div>
      </nav>

      <main className="max-w-7xl mx-auto px-6 py-10">
        {pageError && (
          <div className="mb-6 rounded-xl border border-[rgba(248,113,113,0.35)] bg-[rgba(127,29,29,0.35)] px-5 py-4 text-sm text-[#fecaca]">
            {pageError}
          </div>
        )}

        {tab === "overview" && (
          <div className="space-y-6">
            <SectionCard>
              <h2 className="text-2xl font-bold font-[Sora]">Uebersicht</h2>
              <p className="mt-3 text-sm leading-relaxed text-[#9bb3c5]">
                Willkommen zurueck
                {profile?.display_name ? `, ${profile.display_name}` : ""}. Hier
                verwaltest du deine beanspruchten Streamer, deine Provisionen
                und die automatische Gutschrift-Erstellung.
              </p>
            </SectionCard>

            <ReadinessCard readiness={readiness} compact />

            {needsNextSteps && (
              <SectionCard>
                <h3 className="text-sm font-semibold text-[#9bb3c5] uppercase tracking-wider">
                  Deine naechsten Schritte
                </h3>
                <div className="mt-4 space-y-3">
                  {claims.length === 0 && (
                    <ActionHint
                      icon={<Users size={18} className="text-[#06B6D4] mt-0.5 shrink-0" />}
                      title="Ersten Streamer beanspruchen"
                      text="Oeffne den Tab Streamer und beanspruche einen Channel per Twitch-Login. Ab dann bekommst du 30% auf jede Zahlung dieses Streamers."
                    />
                  )}
                  {!stripeConnected && (
                    <ActionHint
                      icon={
                        <ArrowRightLeft
                          size={18}
                          className="text-[#A855F7] mt-0.5 shrink-0"
                        />
                      }
                      title="Stripe fuer automatische Auszahlungen verbinden"
                      text="Ohne Stripe werden Provisionen nur bis 50,00 EUR gespeichert. Alles darueber verfaellt, bis dein Konto verbunden ist."
                    />
                  )}
                  {!readiness.can_generate && (
                    <ActionHint
                      icon={<ShieldCheck size={18} className="text-[#f5b642] mt-0.5 shrink-0" />}
                      title="Steuer- und Adressdaten vervollstaendigen"
                      text="Pflege im Tab Einstellungen deine Adresse, Kontakt-E-Mail und steuerlichen Angaben, damit Gutschriften automatisch erstellt werden koennen."
                    />
                  )}
                </div>
              </SectionCard>
            )}

            <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-4 gap-6">
              <StatCard
                label="Gesamtverdienst"
                value={formatCents(totalCents)}
                icon={<Receipt size={20} />}
              />
              <StatCard
                label="Ausstehend"
                value={formatCents(pendingCents)}
                icon={<Clock size={20} />}
              />
              <StatCard
                label="Uebertragen"
                value={formatCents(transferredCents)}
                icon={<ArrowRightLeft size={20} />}
              />
              <StatCard
                label="Gutschriften brutto"
                value={formatCents(gutschriftGrossTotal)}
                icon={<FileText size={20} />}
              />
            </div>

            <div className="grid grid-cols-1 xl:grid-cols-[1.2fr_0.8fr] gap-6">
              <SectionCard>
                <h3 className="text-sm font-semibold text-[#9bb3c5] uppercase tracking-wider">
                  Gutschrift-Status
                </h3>
                <div className="mt-4 grid grid-cols-1 md:grid-cols-3 gap-4">
                  <MiniStat
                    label="Dokumente"
                    value={String(gutschriften.length)}
                    tone="orange"
                  />
                  <MiniStat
                    label="Offener Versand"
                    value={String(openGutschriften.length)}
                    tone="teal"
                  />
                  <MiniStat
                    label="Letzte Gutschrift"
                    value={
                      latestGutschrift
                        ? formatCents(latestGutschrift.gross_amount_cents)
                        : "—"
                    }
                    tone="gray"
                  />
                </div>
                <p className="mt-4 text-sm leading-relaxed text-[#9bb3c5]">
                  {taxHint(readiness.ust_status)}
                </p>
              </SectionCard>

              <StripeCard profile={profile} />
            </div>
          </div>
        )}

        {tab === "streamers" && (
          <div className="space-y-6">
            <SectionCard>
              <h2 className="text-2xl font-bold font-[Sora]">Streamer</h2>
              <p className="mt-3 text-sm leading-relaxed text-[#9bb3c5]">
                Beanspruche Streamer ueber ihren Twitch-Login. Sobald ein
                Streamer dir zugeordnet ist, erhaeltst du 30% Provision auf jede
                Zahlung dieses Accounts.
              </p>
            </SectionCard>

            <SectionCard>
              <h3 className="text-sm font-semibold text-[#9bb3c5] uppercase tracking-wider">
                Streamer beanspruchen
              </h3>
              <div className="mt-4 flex flex-col sm:flex-row gap-3">
                <input
                  type="text"
                  placeholder="Twitch-Login eingeben..."
                  value={claimInput}
                  onChange={(event) => setClaimInput(event.target.value)}
                  onKeyDown={(event) => event.key === "Enter" && void handleClaim()}
                  className="flex-1 bg-[#07151d] border border-[rgba(194,221,240,0.14)] rounded-lg px-4 py-2.5 text-sm text-[#e9f1f7] placeholder-[#9bb3c5]/50 focus:outline-none focus:border-[#06B6D4] transition-colors"
                />
                <button
                  onClick={() => {
                    void handleClaim();
                  }}
                  className="gradient-accent rounded-lg px-5 py-2.5 text-sm font-semibold text-white transition-all hover:brightness-110"
                >
                  Beanspruchen
                </button>
              </div>
              {claimMsg && (
                <div
                  className={`mt-3 flex items-center gap-2 text-sm ${
                    claimMsg.ok ? "text-emerald-400" : "text-red-400"
                  }`}
                >
                  {claimMsg.ok ? <CheckCircle size={16} /> : <AlertCircle size={16} />}
                  {claimMsg.text}
                </div>
              )}
            </SectionCard>

            <div className="panel-card rounded-xl overflow-hidden">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-[rgba(194,221,240,0.14)] text-[#9bb3c5] text-left">
                    <th className="px-6 py-3 font-medium">Login</th>
                    <th className="px-6 py-3 font-medium">Anzahl</th>
                    <th className="px-6 py-3 font-medium">Gesamtverdienst</th>
                    <th className="px-6 py-3 font-medium">Beansprucht am</th>
                  </tr>
                </thead>
                <tbody>
                  {claims.map((claim, index) => {
                    const claimLogin =
                      claim.streamer_login ?? claim.claimed_streamer_login ?? "";
                    return (
                      <tr
                        key={`${claimLogin || "claim"}-${claim.claimed_at}-${index}`}
                        className="border-b border-[rgba(194,221,240,0.14)] last:border-0"
                      >
                        <td className="px-6 py-3 font-medium">{claimLogin || "—"}</td>
                        <td className="px-6 py-3">{claim.commission_count}</td>
                        <td className="px-6 py-3">
                          {formatCents(claim.total_commission_cents)}
                        </td>
                        <td className="px-6 py-3 text-[#9bb3c5]">
                          {formatDate(claim.claimed_at)}
                        </td>
                      </tr>
                    );
                  })}
                  {claims.length === 0 && (
                    <tr>
                      <td colSpan={4} className="px-6 py-8 text-center text-[#9bb3c5]">
                        Noch keine Streamer beansprucht.
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
          </div>
        )}

        {tab === "commissions" && (
          <div className="space-y-6">
            <SectionCard>
              <h2 className="text-2xl font-bold font-[Sora]">Provisionen</h2>
              <p className="mt-3 text-sm leading-relaxed text-[#9bb3c5]">
                Jede Zahlung eines von dir beanspruchten Streamers erzeugt eine
                Provision. Hier siehst du den Verlauf inklusive Status.
              </p>
            </SectionCard>

            <div className="panel-card rounded-xl overflow-hidden">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-[rgba(194,221,240,0.14)] text-[#9bb3c5] text-left">
                    <th className="px-6 py-3 font-medium">Datum</th>
                    <th className="px-6 py-3 font-medium">Streamer</th>
                    <th className="px-6 py-3 font-medium">Brutto</th>
                    <th className="px-6 py-3 font-medium">Provision (30%)</th>
                    <th className="px-6 py-3 font-medium">Status</th>
                    <th className="px-6 py-3 font-medium">Referenz</th>
                  </tr>
                </thead>
                <tbody>
                  {commissions.map((commission) => (
                    <tr
                      key={commission.id}
                      className="border-b border-[rgba(194,221,240,0.14)] last:border-0"
                    >
                      <td className="px-6 py-3 text-[#9bb3c5]">
                        {formatDate(commission.created_at)}
                      </td>
                      <td className="px-6 py-3 font-medium">{commission.streamer_login}</td>
                      <td className="px-6 py-3">{formatCents(commission.brutto_cents)}</td>
                      <td className="px-6 py-3 text-[#A855F7] font-medium">
                        {formatCents(commission.commission_cents)}
                      </td>
                      <td className="px-6 py-3">
                        <StatusBadge status={commission.status} />
                      </td>
                      <td className="px-6 py-3 text-[#9bb3c5] font-mono text-xs">
                        #{commission.id}
                      </td>
                    </tr>
                  ))}
                  {commissions.length === 0 && (
                    <tr>
                      <td colSpan={6} className="px-6 py-8 text-center text-[#9bb3c5]">
                        Noch keine Provisionen vorhanden.
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
          </div>
        )}

        {tab === "gutschriften" && (
          <div className="space-y-6">
            <SectionCard>
              <h2 className="text-2xl font-bold font-[Sora]">Gutschriften</h2>
              <p className="mt-3 text-sm leading-relaxed text-[#9bb3c5]">
                Monatliche Gutschriften werden automatisch auf Basis deiner
                Provisionen erzeugt und per E-Mail versendet, sobald dein Profil
                vollständig ist.
              </p>
            </SectionCard>

            <ReadinessCard readiness={readiness} />

            <div className="grid grid-cols-1 md:grid-cols-3 gap-6">
              <StatCard
                label="Dokumente"
                value={String(gutschriften.length)}
                icon={<FileText size={20} />}
              />
              <StatCard
                label="Offener Versand"
                value={String(openGutschriften.length)}
                icon={<Clock size={20} />}
              />
              <StatCard
                label="Brutto gesamt"
                value={formatCents(gutschriftGrossTotal)}
                icon={<ShieldCheck size={20} />}
              />
            </div>

            <SectionCard>
              <h3 className="text-sm font-semibold text-[#9bb3c5] uppercase tracking-wider">
                Steuerhinweis
              </h3>
              <p className="mt-3 text-sm leading-relaxed text-[#9bb3c5]">
                {taxHint(readiness.ust_status)}
              </p>
              {readiness.warnings.length > 0 && (
                <div className="mt-4 rounded-xl border border-[rgba(245,182,66,0.25)] bg-[rgba(120,53,15,0.35)] px-4 py-3">
                  {readiness.warnings.map((warning) => (
                    <div key={warning} className="flex items-start gap-2 text-sm text-[#fde68a]">
                      <AlertCircle size={16} className="mt-0.5 shrink-0" />
                      <span>{warning}</span>
                    </div>
                  ))}
                </div>
              )}
            </SectionCard>

            <div className="panel-card rounded-xl overflow-hidden">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-[rgba(194,221,240,0.14)] text-[#9bb3c5] text-left">
                    <th className="px-6 py-3 font-medium">Zeitraum</th>
                    <th className="px-6 py-3 font-medium">Nummer</th>
                    <th className="px-6 py-3 font-medium">Buchungen</th>
                    <th className="px-6 py-3 font-medium">Netto</th>
                    <th className="px-6 py-3 font-medium">USt</th>
                    <th className="px-6 py-3 font-medium">Brutto</th>
                    <th className="px-6 py-3 font-medium">Status</th>
                    <th className="px-6 py-3 font-medium">Versand</th>
                    <th className="px-6 py-3 font-medium text-right">PDF</th>
                  </tr>
                </thead>
                <tbody>
                  {gutschriften.map((document) => (
                    <tr
                      key={document.id}
                      className="border-b border-[rgba(194,221,240,0.14)] last:border-0"
                    >
                      <td className="px-6 py-4">
                        <div className="font-medium">{document.period_label || "—"}</div>
                        <div className="text-xs text-[#9bb3c5]">
                          erstellt: {formatDate(document.generated_at)}
                        </div>
                      </td>
                      <td className="px-6 py-4 font-mono text-xs text-[#d7e5f0]">
                        {document.gutschrift_number || "noch offen"}
                      </td>
                      <td className="px-6 py-4">{document.commission_count}</td>
                      <td className="px-6 py-4">
                        {formatCents(document.net_amount_cents)}
                      </td>
                      <td className="px-6 py-4">
                        {formatCents(document.vat_amount_cents)}
                      </td>
                      <td className="px-6 py-4 font-medium text-[#A855F7]">
                        {formatCents(document.gross_amount_cents)}
                      </td>
                      <td className="px-6 py-4">
                        <StatusBadge status={document.status} />
                      </td>
                      <td className="px-6 py-4 text-[#9bb3c5]">
                        {document.status === "email_failed" && document.last_error
                          ? document.last_error
                          : formatDateTime(document.emailed_at)}
                      </td>
                      <td className="px-6 py-4">
                        <div className="flex justify-end">
                          {document.has_pdf && document.download_path ? (
                            <button
                              onClick={() => {
                                window.location.href = document.download_path as string;
                              }}
                              className="inline-flex items-center gap-2 rounded-lg border border-[rgba(194,221,240,0.14)] bg-[#102635] px-3 py-2 text-xs font-semibold text-[#e9f1f7] transition-colors hover:border-[rgba(255,122,24,0.5)] hover:text-[#ffb067]"
                            >
                              <Download size={14} />
                              PDF
                            </button>
                          ) : (
                            <span className="text-[#6c8394]">—</span>
                          )}
                        </div>
                      </td>
                    </tr>
                  ))}
                  {gutschriften.length === 0 && (
                    <tr>
                      <td colSpan={9} className="px-6 py-8 text-center text-[#9bb3c5]">
                        Noch keine Gutschriften vorhanden.
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
          </div>
        )}

        {tab === "settings" && profile && (
          <div className="space-y-6">
            <SectionCard>
              <h2 className="text-2xl font-bold font-[Sora]">Einstellungen</h2>
              <p className="mt-3 text-sm leading-relaxed text-[#9bb3c5]">
                Pflege hier deine verschluesselten Profil-, Adress- und
                Steuerdaten fuer die automatische Gutschrift-Erstellung.
              </p>
            </SectionCard>

            <ReadinessCard readiness={readiness} compact />

            <form onSubmit={handleProfileSave} className="space-y-6">
              <SectionCard>
                <div className="flex flex-col md:flex-row md:items-start md:justify-between gap-4">
                  <div>
                    <h3 className="text-sm font-semibold text-[#9bb3c5] uppercase tracking-wider">
                      Adresse
                    </h3>
                    <p className="mt-2 text-sm leading-relaxed text-[#9bb3c5]">
                      Diese Daten werden verschluesselt gespeichert und fuer die
                      automatische Gutschrift-Erstellung benoetigt.
                    </p>
                  </div>
                  <div className="rounded-xl border border-[rgba(16,183,173,0.22)] bg-[rgba(16,183,173,0.08)] px-4 py-3 text-sm text-[#d8f8f4] max-w-md">
                    Diese Daten werden verschluesselt gespeichert und fuer die
                    automatische Gutschrift-Erstellung benoetigt.
                  </div>
                </div>

                <div className="mt-6 grid grid-cols-1 md:grid-cols-2 gap-4">
                  <InputField
                    label="Vollstaendiger Name"
                    value={settingsForm.full_name}
                    onChange={(value) => updateForm("full_name", value)}
                  />
                  <InputField
                    label="Kontakt-E-Mail"
                    type="email"
                    value={settingsForm.email}
                    onChange={(value) => updateForm("email", value)}
                  />
                  <InputField
                    label="Strasse"
                    value={settingsForm.address_line1}
                    onChange={(value) => updateForm("address_line1", value)}
                  />
                  <SelectField
                    label="Land"
                    value={settingsForm.address_country}
                    onChange={(value) => updateForm("address_country", value)}
                    options={COUNTRIES}
                  />
                  <InputField
                    label="PLZ"
                    value={settingsForm.address_zip}
                    onChange={(value) => updateForm("address_zip", value)}
                  />
                  <InputField
                    label="Ort"
                    value={settingsForm.address_city}
                    onChange={(value) => updateForm("address_city", value)}
                  />
                </div>
              </SectionCard>

              <SectionCard>
                <h3 className="text-sm font-semibold text-[#9bb3c5] uppercase tracking-wider">
                  Steuerliche Angaben
                </h3>
                <div className="mt-5 grid grid-cols-1 lg:grid-cols-[1.05fr_0.95fr] gap-6">
                  <div className="space-y-3">
                    <span className="text-xs text-[#9bb3c5] uppercase tracking-wider">
                      USt-Status
                    </span>
                    <label className="flex items-start gap-3 rounded-xl border border-[rgba(194,221,240,0.14)] bg-[#102635]/40 px-4 py-3 cursor-pointer">
                      <input
                        type="radio"
                        name="ust_status"
                        checked={settingsForm.ust_status === "unknown"}
                        onChange={() => updateForm("ust_status", "unknown")}
                        className="mt-1 accent-[#06B6D4]"
                      />
                      <div>
                        <div className="font-medium">Noch nicht angegeben</div>
                        <div className="text-sm text-[#9bb3c5]">
                          Gutschrift-Generierung bleibt blockiert.
                        </div>
                      </div>
                    </label>
                    <label className="flex items-start gap-3 rounded-xl border border-[rgba(194,221,240,0.14)] bg-[#102635]/40 px-4 py-3 cursor-pointer">
                      <input
                        type="radio"
                        name="ust_status"
                        checked={settingsForm.ust_status === "kleinunternehmer"}
                        onChange={() => updateForm("ust_status", "kleinunternehmer")}
                        className="mt-1 accent-[#06B6D4]"
                      />
                      <div>
                        <div className="font-medium">Kleinunternehmer</div>
                        <div className="text-sm text-[#9bb3c5]">
                          Keine USt-Zeile, Hinweis nach § 19 UStG.
                        </div>
                      </div>
                    </label>
                    <label className="flex items-start gap-3 rounded-xl border border-[rgba(194,221,240,0.14)] bg-[#102635]/40 px-4 py-3 cursor-pointer">
                      <input
                        type="radio"
                        name="ust_status"
                        checked={settingsForm.ust_status === "regelbesteuert"}
                        onChange={() => updateForm("ust_status", "regelbesteuert")}
                        className="mt-1 accent-[#06B6D4]"
                      />
                      <div>
                        <div className="font-medium">Regelbesteuert</div>
                        <div className="text-sm text-[#9bb3c5]">
                          19 % Umsatzsteuer werden auf den Nettobetrag ausgewiesen.
                        </div>
                      </div>
                    </label>
                  </div>

                  <div className="space-y-4">
                    <InputField
                      label="Steuernummer"
                      value={settingsForm.tax_id}
                      onChange={(value) => updateForm("tax_id", value)}
                    />
                    {settingsForm.ust_status === "regelbesteuert" && (
                      <InputField
                        label="USt-IdNr."
                        value={settingsForm.vat_id}
                        onChange={(value) => updateForm("vat_id", value)}
                      />
                    )}
                    <div className="rounded-xl border border-[rgba(255,122,24,0.2)] bg-[rgba(255,122,24,0.08)] px-4 py-3 text-sm text-[#ffd8b5]">
                      {taxHint(settingsForm.ust_status)}
                    </div>
                  </div>
                </div>
              </SectionCard>

              <SectionCard>
                <h3 className="text-sm font-semibold text-[#9bb3c5] uppercase tracking-wider">
                  Konto
                </h3>
                <div className="mt-4 grid grid-cols-1 md:grid-cols-2 xl:grid-cols-4 gap-4">
                  <InfoField label="Twitch Login" value={profile.twitch_login} />
                  <InfoField label="Anzeigename" value={profile.display_name} />
                  <InfoField label="Stripe Status" value={profile.stripe_connect_status} />
                  <InfoField
                    label="Stripe Konto"
                    value={profile.stripe_account_id || "Nicht verbunden"}
                  />
                </div>
              </SectionCard>

              <div className="flex flex-col sm:flex-row sm:items-center gap-3">
                <button
                  type="submit"
                  disabled={savingProfile}
                  className="gradient-accent rounded-lg px-5 py-3 text-sm font-semibold text-white transition-all hover:brightness-110 disabled:opacity-60 disabled:cursor-not-allowed"
                >
                  {savingProfile ? "Speichert..." : "Einstellungen speichern"}
                </button>
                {profileMsg && (
                  <div
                    className={`flex items-center gap-2 text-sm ${
                      profileMsg.ok ? "text-emerald-400" : "text-red-400"
                    }`}
                  >
                    {profileMsg.ok ? <CheckCircle size={16} /> : <AlertCircle size={16} />}
                    {profileMsg.text}
                  </div>
                )}
              </div>
            </form>

            <StripeCard profile={profile} />
          </div>
        )}
      </main>
    </div>
  );
}

function SectionCard({ children }: { children: ReactNode }) {
  return <div className="panel-card rounded-xl p-6">{children}</div>;
}

function StatCard({
  label,
  value,
  icon,
}: {
  label: string;
  value: string;
  icon: ReactNode;
}) {
  return (
    <div className="panel-card rounded-xl p-6">
      <div className="flex items-center justify-between mb-2">
        <span className="text-sm text-[#9bb3c5]">{label}</span>
        <span className="text-[#06B6D4]">{icon}</span>
      </div>
      <p className="text-2xl font-bold font-[Sora]">{value}</p>
    </div>
  );
}

function MiniStat({
  label,
  value,
  tone,
}: {
  label: string;
  value: string;
  tone: "orange" | "teal" | "gray";
}) {
  const tones: Record<string, string> = {
    orange: "text-[#ffb067] border-[rgba(255,122,24,0.18)]",
    teal: "text-[#7ce9df] border-[rgba(16,183,173,0.22)]",
    gray: "text-[#e9f1f7] border-[rgba(194,221,240,0.14)]",
  };
  return (
    <div className={`rounded-xl border bg-[#102635]/40 px-4 py-4 ${tones[tone]}`}>
      <div className="text-xs uppercase tracking-wider text-[#9bb3c5]">{label}</div>
      <div className="mt-2 text-xl font-semibold font-[Sora]">{value}</div>
    </div>
  );
}

function ActionHint({
  icon,
  title,
  text,
}: {
  icon: ReactNode;
  title: string;
  text: string;
}) {
  return (
    <div className="flex items-start gap-3 rounded-xl border border-[rgba(194,221,240,0.14)] bg-[#102635]/40 px-4 py-3">
      {icon}
      <div>
        <p className="text-sm font-semibold text-[#e9f1f7]">{title}</p>
        <p className="mt-1 text-sm leading-relaxed text-[#9bb3c5]">{text}</p>
      </div>
    </div>
  );
}

function ReadinessCard({
  readiness,
  compact = false,
}: {
  readiness: GutschriftReadiness;
  compact?: boolean;
}) {
  const hasIssues = readiness.blockers.length > 0 || readiness.warnings.length > 0;
  if (!hasIssues && compact) {
    return null;
  }

  return (
    <SectionCard>
      <div className="flex items-start justify-between gap-4">
        <div>
          <h3 className="text-sm font-semibold text-[#9bb3c5] uppercase tracking-wider">
            Gutschrift-Bereitschaft
          </h3>
          <p className="mt-2 text-sm leading-relaxed text-[#9bb3c5]">
            {readiness.can_generate
              ? "Dein Profil ist bereit fuer die automatische Gutschrift-Erstellung."
              : "Dein Profil blockiert aktuell die automatische Gutschrift-Erstellung."}
          </p>
        </div>
        <StatusBadge status={readiness.can_generate ? "generated" : "blocked"} />
      </div>

      {readiness.blockers.length > 0 && (
        <div className="mt-4 rounded-xl border border-[rgba(245,182,66,0.28)] bg-[rgba(120,53,15,0.35)] px-4 py-4">
          {readiness.blockers.map((blocker) => (
            <div key={blocker} className="flex items-start gap-2 text-sm text-[#fde68a]">
              <AlertCircle size={16} className="mt-0.5 shrink-0" />
              <span>{blocker}</span>
            </div>
          ))}
        </div>
      )}

      {readiness.warnings.length > 0 && (
        <div className="mt-4 space-y-2">
          {readiness.warnings.map((warning) => (
            <div
              key={warning}
              className="flex items-start gap-2 rounded-xl border border-[rgba(194,221,240,0.14)] bg-[#102635]/40 px-4 py-3 text-sm text-[#d7e5f0]"
            >
              <AlertCircle size={16} className="mt-0.5 shrink-0 text-[#ffb067]" />
              <span>{warning}</span>
            </div>
          ))}
        </div>
      )}
    </SectionCard>
  );
}

function StripeCard({ profile }: { profile: AffiliateProfile | null }) {
  if (!profile) return null;
  const connected = profile.stripe_connect_status === "connected";
  return (
    <div className="panel-card rounded-xl p-6">
      <h3 className="text-sm font-semibold text-[#9bb3c5] uppercase tracking-wider mb-4">
        Stripe Connect
      </h3>
      <div className="space-y-4">
        <p className="text-sm leading-relaxed text-[#9bb3c5]">
          Auszahlungen laufen automatisch ueber Stripe Connect auf dein
          verbundenes Auszahlungs-Konto.
        </p>
        {connected ? (
          <div className="space-y-2">
            <div className="flex items-center gap-2 text-emerald-400">
              <CheckCircle size={18} />
              <span className="font-medium">Verbunden</span>
            </div>
            <p className="text-sm leading-relaxed text-[#9bb3c5]">
              Neue Provisionen koennen automatisch fuer Auszahlungen
              verarbeitet werden, sobald Stripe Transfers ausloest.
            </p>
          </div>
        ) : (
          <div className="space-y-3">
            <div className="flex items-center gap-2 text-yellow-400">
              <XCircle size={18} />
              <span className="text-sm">Nicht verbunden</span>
            </div>
            <p className="text-sm leading-relaxed text-[#9bb3c5]">
              Ohne Stripe werden Provisionen nur bis 50,00 EUR gespeichert.
              Sobald weitere Provisionen darueber hinaus anfallen, verfallen
              sie, bis du Stripe verbunden hast.
            </p>
            <button
              onClick={() => {
                window.location.href = "/twitch/affiliate/connect/stripe";
              }}
              className="gradient-accent rounded-lg px-5 py-2.5 text-sm font-semibold text-white transition-all hover:brightness-110"
            >
              Stripe-Konto verbinden
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

function InfoField({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <span className="text-xs text-[#9bb3c5] uppercase tracking-wider">{label}</span>
      <p className="text-sm font-medium mt-1">{value || "—"}</p>
    </div>
  );
}

function InputField({
  label,
  value,
  onChange,
  type = "text",
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  type?: string;
}) {
  return (
    <label className="block">
      <span className="text-xs text-[#9bb3c5] uppercase tracking-wider">{label}</span>
      <input
        type={type}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        className="mt-2 w-full bg-[#07151d] border border-[rgba(194,221,240,0.14)] rounded-lg px-4 py-2.5 text-sm text-[#e9f1f7] placeholder-[#9bb3c5]/50 focus:outline-none focus:border-[#06B6D4] transition-colors"
      />
    </label>
  );
}

function SelectField({
  label,
  value,
  onChange,
  options,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  options: { value: string; label: string }[];
}) {
  return (
    <label className="block">
      <span className="text-xs text-[#9bb3c5] uppercase tracking-wider">{label}</span>
      <select
        value={value}
        onChange={(event) => onChange(event.target.value)}
        className="mt-2 w-full bg-[#07151d] border border-[rgba(194,221,240,0.14)] rounded-lg px-4 py-2.5 text-sm text-[#e9f1f7] focus:outline-none focus:border-[#06B6D4] transition-colors"
      >
        {options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
    </label>
  );
}
