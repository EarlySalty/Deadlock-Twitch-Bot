import { ArrowLeft, UserPlus } from "lucide-react";
import { WEBSITE_HOME_PATH } from "@/data/sitePaths";

export function AffiliateNavbar() {
  return (
    <header className="fixed top-0 left-0 right-0 z-50 glass">
      <div className="max-w-7xl mx-auto px-6 h-16 flex items-center justify-between gap-4">
        <div className="flex items-center gap-4 min-w-0">
          <a
            href={WEBSITE_HOME_PATH}
            className="font-display font-bold text-xl bg-gradient-to-r from-primary to-accent bg-clip-text text-transparent select-none shrink-0"
          >
            DDC
          </a>
          <a
            href={WEBSITE_HOME_PATH}
            className="hidden sm:inline-flex items-center gap-2 text-sm text-text-secondary hover:text-text-primary transition-colors duration-200"
          >
            <ArrowLeft size={16} />
            Zur Startseite
          </a>
        </div>

        <a
          href="/twitch/affiliate/signup"
          className="gradient-accent rounded-lg px-4 py-2 text-sm font-semibold text-white inline-flex items-center gap-2 whitespace-nowrap"
        >
          <UserPlus size={16} />
          Jetzt Vertriebler werden
        </a>
      </div>
    </header>
  );
}
