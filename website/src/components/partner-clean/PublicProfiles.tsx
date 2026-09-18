import { useEffect, useState } from "react";

import { parsePublicProfiles, type PublicProfile } from "@/lib/publicProfiles";

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
  return <section aria-labelledby="public-profiles-heading" className="mt-10 rounded-2xl border border-border bg-white/[0.02] p-5 sm:p-7">
    <p className="text-xs font-semibold uppercase tracking-widest text-primary">Lern uns kennen</p>
    <h3 id="public-profiles-heading" className="mt-2 text-2xl font-semibold text-text-primary">Die Menschen hinter den Streams</h3>
    <p className="mt-2 text-sm text-text-secondary">Eigene Profile, Socials und Streamkalender. Entdecke deinen nächsten Lieblingsstream.</p>
    <div className="mt-5 grid gap-3 sm:grid-cols-2 lg:grid-cols-3">{(expanded ? profiles : profiles.slice(0, 6)).map(profile => <a key={profile.login} href={`/streamer/${profile.login}`} className="rounded-xl border border-border p-4 transition-colors hover:border-primary/40 focus-visible:outline-primary">
      <span className="font-semibold text-primary">@{profile.login} ↗</span>
      <p className="mt-2 break-words text-sm text-text-secondary">{profile.headline || "Profil und Streamkalender entdecken"}</p>
    </a>)}</div>
    {profiles.length > 6 && <button type="button" aria-expanded={expanded} className="mt-5 text-sm font-semibold text-primary" onClick={() => setExpanded(value => !value)}>{expanded ? "Weniger zeigen" : `Alle ${profiles.length} Profile entdecken`}</button>}
  </section>;
}
