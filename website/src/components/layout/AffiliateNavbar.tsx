import {
  landingText,
  landingLink,
  isEnglishLanding,
} from "@/lib/landingLocale";
import { ArrowLeft, UserPlus } from "lucide-react";
import { WEBSITE_HOME_PATH } from "@/data/sitePaths";

export function AffiliateNavbar() {
  const english = isEnglishLanding();
  return (
    <header className="fixed top-0 left-0 right-0 z-50 glass">
      <div className="max-w-7xl mx-auto px-6 h-16 flex items-center justify-between gap-4">
        <div className="flex items-center gap-4 min-w-0">
          <a
            href={landingLink(WEBSITE_HOME_PATH)}
            className={`flex items-center gap-2.5 select-none ${english ? "min-w-0" : "shrink-0"}`}
          >
            <img
              src={`${import.meta.env.BASE_URL}brand/deadlock-d-logo.png`}
              alt=""
              className="h-8 w-8 shrink-0"
            />
            <span
              className={`font-display font-bold ${english ? "text-sm sm:text-xl" : "text-xl"} bg-gradient-to-r from-primary to-accent bg-clip-text text-transparent`}
            >
              {landingText("Deutsche Deadlock Community")}
            </span>
          </a>
          <a
            href={landingLink(WEBSITE_HOME_PATH)}
            className="hidden sm:inline-flex items-center gap-2 text-sm text-text-secondary hover:text-text-primary transition-colors duration-200"
          >
            <ArrowLeft size={16} />
            {landingText("Zur Startseite")}
          </a>
        </div>

        <a
          href={landingLink("/twitch/affiliate/signup")}
          className={`gradient-accent rounded-lg ${english ? "px-3 sm:px-4 text-xs sm:text-sm shrink-0" : "px-4 text-sm"} py-2 font-semibold inline-flex items-center gap-2 whitespace-nowrap`}
        >
          <UserPlus size={16} />
          {landingText("Jetzt Vertriebler werden")}
        </a>
      </div>
    </header>
  );
}
