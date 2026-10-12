import { useEffect, useState } from "react";

import { parsePublicProfiles, type PublicProfile } from "@/lib/publicProfiles";
import { tx } from "@/i18n/locale";

export function PublicProfiles() {
  const [profiles, setProfiles] = useState<PublicProfile[]>([]);
  const [expanded, setExpanded] = useState(false);
  useEffect(() => {
    let disposed = false;
    let controller: AbortController | undefined;
    let generation = 0;
    async function refresh() {
      controller?.abort();
      const request = new AbortController();
      controller = request;
      const current = ++generation;
      const timer = window.setTimeout(() => request.abort(), 8000);
      try {
        const response = await fetch("/twitch/api/v2/public/partner-profiles", { cache: "no-store", credentials: "omit", signal: request.signal });
        if (!response.ok) throw new Error("Profiles unavailable");
        const data: unknown = await response.json();
        if (!disposed && current === generation) setProfiles(parsePublicProfiles(data));
      } catch { if (!disposed && current === generation) setProfiles([]); }
      finally { window.clearTimeout(timer); }
    }
    void refresh();
    const interval = window.setInterval(() => { if (document.visibilityState === "visible") void refresh(); }, 60_000);
    const onVisible = () => { if (document.visibilityState === "visible") void refresh(); };
    document.addEventListener("visibilitychange", onVisible);
    return () => { disposed = true; controller?.abort(); window.clearInterval(interval); document.removeEventListener("visibilitychange", onVisible); };
  }, []);
  if (!profiles.length) return null;
  return <div className="mx-auto max-w-[1600px] px-6"><section aria-labelledby="public-profiles-heading" className="mt-10 rounded-2xl border border-border bg-white/[0.02] p-5 sm:p-7">
    <p className="text-xs font-semibold uppercase tracking-widest text-primary">{tx("Lern uns kennen", "Get to know us")}</p>
    <h3 id="public-profiles-heading" className="mt-2 text-2xl font-semibold text-text-primary">{tx("Die Menschen hinter den Streams", "The people behind the streams")}</h3>
    <p className="mt-2 text-sm text-text-secondary">{tx("Eigene Profile, Socials und Streamkalender. Entdecke deinen nächsten Lieblingsstream.", "Personal profiles, socials and stream schedules. Find your next favorite stream.")}</p>
    <div className="mt-5 grid gap-3 sm:grid-cols-2 lg:grid-cols-3">{(expanded ? profiles : profiles.slice(0, 6)).map(profile => <a key={profile.login} href={`/streamer/${profile.login}`} className="rounded-xl border border-border p-4 transition-colors hover:border-primary/40 focus-visible:outline-primary">
      <span className="font-semibold text-primary">@{profile.login} ↗</span>
      <p className="mt-2 break-words text-sm text-text-secondary">{profile.headline || tx("Profil und Streamkalender entdecken", "Check out profile and stream schedule")}</p>
    </a>)}</div>
    {profiles.length > 6 && <button type="button" aria-expanded={expanded} className="mt-5 text-sm font-semibold text-primary" onClick={() => setExpanded(value => !value)}>{expanded ? tx("Weniger zeigen", "Show less") : tx(`Alle ${profiles.length} Profile entdecken`, `Explore all ${profiles.length} profiles`)}</button>}
  </section></div>;
}
