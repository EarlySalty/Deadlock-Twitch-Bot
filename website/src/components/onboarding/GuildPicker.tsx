import { useEffect, useMemo, useState } from "react";
import { TWITCH_PUBLIC_ORIGIN } from "@/data/externalLinks";

type Locale = "de" | "en";

type Guild = {
  guild_id: string;
  name: string;
  language: string;
  auto_partner_default: boolean;
};

type GuildsResponse = {
  needs_discord: boolean;
  decision: "login" | "none" | "auto" | "pick";
  guilds: Guild[];
  assigned: Guild | null;
};

const COPY: Record<
  Locale,
  {
    title: string;
    connect: string;
    none: string;
    assigned: string;
    pick: string;
    wait: string;
    submit: string;
    error: string;
    foreign: string;
    loading: string;
  }
> = {
  de: {
    title: "Wähle deinen Discord-Server",
    connect: "Mit Discord verbinden",
    none: "Bitte den Server-Inhaber, den Deadlock-Hub einzuladen.",
    assigned: "Du bist diesem Server zugeordnet:",
    pick: "Wähle den Server, auf dem du streamst.",
    wait: "Dieser Server nimmt neue Partner nicht automatisch auf. Ein Admin schaltet dich frei.",
    submit: "Server wählen",
    error: "Das hat nicht geklappt. Bitte versuche es erneut.",
    foreign: "Du kannst nur einen Server wählen, in dem du selbst Mitglied bist.",
    loading: "Server werden geladen …",
  },
  en: {
    title: "Choose your Discord server",
    connect: "Connect with Discord",
    none: "Ask the server owner to add the Deadlock Hub.",
    assigned: "You are assigned to this server:",
    pick: "Choose the server you stream on.",
    wait: "This server does not add new partners automatically. A server admin will activate you.",
    submit: "Choose server",
    error: "That did not work. Please try again.",
    foreign: "You can only choose a server you belong to.",
    loading: "Loading servers…",
  },
};

function hostLocale(): Locale {
  if (typeof window === "undefined") {
    return "de";
  }
  const query = new URLSearchParams(window.location.search).get("locale");
  if (query === "en" || query === "de") {
    return query;
  }
  const host = window.location.hostname.toLowerCase();
  if (host === "dead-lock.de" || host.endsWith(".dead-lock.de")) {
    return "en";
  }
  return "de";
}

function apiOrigin(): string {
  if (typeof window === "undefined") {
    return TWITCH_PUBLIC_ORIGIN;
  }
  const host = window.location.hostname.toLowerCase();
  if (host === "dead-lock.de" || host.endsWith(".dead-lock.de")) {
    return TWITCH_PUBLIC_ORIGIN;
  }
  return window.location.origin;
}

function discordLoginUrl(locale: Locale): string {
  const url = new URL("/twitch/onboarding/discord/login", apiOrigin());
  url.searchParams.set("locale", locale);
  if (typeof window !== "undefined") {
    url.searchParams.set("return", window.location.href.split("#")[0]);
  }
  return url.toString();
}

export function GuildPicker() {
  const locale = useMemo(hostLocale, []);
  const copy = COPY[locale];
  const [status, setStatus] = useState<"loading" | "ready" | "error">("loading");
  const [payload, setPayload] = useState<GuildsResponse | null>(null);
  const [selected, setSelected] = useState<string>("");
  const [saving, setSaving] = useState(false);
  const [assignedName, setAssignedName] = useState<string>("");
  const [inactive, setInactive] = useState(false);
  const [errorKind, setErrorKind] = useState<"error" | "foreign">("error");

  useEffect(() => {
    const query = new URLSearchParams(window.location.search);
    const onboarding = query.get("onboarding");
    if (onboarding === "assigned") {
      setInactive(query.get("inactive") === "1");
      setStatus("ready");
      setPayload({
        needs_discord: false,
        decision: "auto",
        guilds: [],
        assigned: null,
      });
      return;
    }
    if (onboarding === "none") {
      setStatus("ready");
      setPayload({
        needs_discord: false,
        decision: "none",
        guilds: [],
        assigned: null,
      });
      return;
    }
    if (onboarding === "foreign") {
      setErrorKind("foreign");
      setStatus("error");
      return;
    }
    if (onboarding === "error") {
      setStatus("error");
      return;
    }
    fetch(new URL("/twitch/api/v2/streamer/onboarding/guilds", apiOrigin()), {
      credentials: "include",
      headers: { Accept: "application/json" },
    })
      .then(async (response) => {
        if (response.status === 401) {
          setPayload({
            needs_discord: true,
            decision: "login",
            guilds: [],
            assigned: null,
          });
          setStatus("ready");
          return;
        }
        if (!response.ok) {
          setStatus("error");
          return;
        }
        const body = (await response.json()) as GuildsResponse;
        if (body.decision === "auto" && body.assigned?.guild_id) {
          setStatus("ready");
          await assign(body.assigned.guild_id);
          return;
        }
        setPayload(body);
        if (body.guilds.length === 1) {
          setSelected(body.guilds[0].guild_id);
        }
        setStatus("ready");
      })
      .catch(() => setStatus("error"));
  }, []);

  async function assign(guildId: string) {
    setSaving(true);
    try {
      const response = await fetch(
        new URL("/twitch/api/v2/streamer/onboarding/guilds", apiOrigin()),
        {
          method: "POST",
          credentials: "include",
          headers: {
            Accept: "application/json",
            "Content-Type": "application/json",
          },
          body: JSON.stringify({ guild_id: guildId }),
        },
      );
      if (!response.ok) {
        setErrorKind(response.status === 403 ? "foreign" : "error");
        setStatus("error");
        return;
      }
      const body = (await response.json()) as {
        name?: string;
        active?: boolean;
      };
      setAssignedName(body.name ?? "");
      setInactive(body.active === false);
      setPayload({
        needs_discord: false,
        decision: "auto",
        guilds: [],
        assigned: null,
      });
    } catch {
      setStatus("error");
    } finally {
      setSaving(false);
    }
  }

  return (
    <section className="px-6 pb-8">
      <div className="mx-auto max-w-3xl rounded-2xl border border-border bg-[rgba(44,35,24,0.55)] p-6">
        <h2 className="text-2xl font-semibold text-text-primary">{copy.title}</h2>
        {status === "loading" ? (
          <p className="mt-3 text-text-secondary">{copy.loading}</p>
        ) : null}
        {status === "error" ? (
          <p className="mt-3 text-text-secondary">
            {errorKind === "foreign" ? copy.foreign : copy.error}
          </p>
        ) : null}
        {status === "ready" && payload?.needs_discord ? (
          <a
            href={discordLoginUrl(locale)}
            className="gradient-accent mt-5 inline-flex rounded-xl px-5 py-3 font-semibold no-underline"
          >
            {copy.connect}
          </a>
        ) : null}
        {status === "ready" && payload?.decision === "none" ? (
          <p className="mt-3 text-text-secondary">{copy.none}</p>
        ) : null}
        {status === "ready" && (payload?.decision === "auto" || assignedName) ? (
          <p className="mt-3 text-text-secondary">
            {copy.assigned} {assignedName || payload?.assigned?.name || ""}
            {inactive || payload?.assigned?.auto_partner_default === false ? (
              <span className="mt-2 block">{copy.wait}</span>
            ) : null}
          </p>
        ) : null}
        {status === "ready" && payload?.decision === "pick" ? (
          <form
            className="mt-4 flex flex-col gap-3"
            onSubmit={(event) => {
              event.preventDefault();
              if (selected) {
                void assign(selected);
              }
            }}
          >
            <p className="text-text-secondary">{copy.pick}</p>
            {payload.guilds.map((guild) => (
              <label key={guild.guild_id} className="flex items-center gap-3 text-text-primary">
                <input
                  type="radio"
                  name="guild"
                  value={guild.guild_id}
                  checked={selected === guild.guild_id}
                  onChange={() => setSelected(guild.guild_id)}
                />
                {guild.name}
              </label>
            ))}
            <button
              type="submit"
              disabled={!selected || saving}
              className="gradient-accent mt-2 inline-flex w-fit rounded-xl px-5 py-3 font-semibold"
            >
              {copy.submit}
            </button>
          </form>
        ) : null}
      </div>
    </section>
  );
}
