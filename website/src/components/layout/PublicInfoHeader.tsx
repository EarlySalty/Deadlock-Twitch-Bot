import { useEffect, useState } from "react";
import { Menu, X } from "lucide-react";

interface NavLink {
  label: string;
  href: string;
}

interface ActionLink {
  label: string;
  href: string;
  variant?: "primary" | "ghost";
  onClick?: () => void;
}

interface PublicInfoHeaderProps {
  navLinks: NavLink[];
  primaryAction: ActionLink;
  secondaryAction?: ActionLink;
}

function ActionButton({ action }: { action: ActionLink }) {
  const baseClass =
    "inline-flex items-center justify-center rounded-xl px-4 py-2 text-sm font-semibold transition-all duration-200 no-underline";
  const variantClass =
    action.variant === "ghost"
      ? "border border-border text-text-primary hover:border-border-hover hover:bg-white/5"
      : "gradient-accent text-white hover:brightness-110";

  return (
    <a
      href={action.href}
      className={`${baseClass} ${variantClass}`}
      onClick={action.onClick}
    >
      {action.label}
    </a>
  );
}

export function PublicInfoHeader({
  navLinks,
  primaryAction,
  secondaryAction,
}: PublicInfoHeaderProps) {
  const [glassy, setGlassy] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);

  useEffect(() => {
    function handleScroll() {
      setGlassy(window.scrollY > 24);
    }

    function handleResize() {
      if (window.innerWidth >= 768) {
        setMenuOpen(false);
      }
    }

    window.addEventListener("scroll", handleScroll, { passive: true });
    window.addEventListener("resize", handleResize);
    return () => {
      window.removeEventListener("scroll", handleScroll);
      window.removeEventListener("resize", handleResize);
    };
  }, []);

  return (
    <header
      className={`fixed top-0 left-0 right-0 z-50 transition-all duration-300 ${glassy ? "glass" : ""}`}
    >
      <div className="mx-auto flex h-16 max-w-7xl items-center justify-between px-6">
        <a
          href="/twitch/onboarding"
          className="bg-gradient-to-r from-primary to-accent bg-clip-text text-xl font-bold font-display text-transparent no-underline"
        >
          Deutsche Deadlock Community
</a>

        <nav className="hidden items-center gap-6 md:flex">
          {navLinks.map((link) => (
            <a
              key={link.href}
              href={link.href}
              className="text-sm font-medium text-text-secondary transition-colors duration-200 hover:text-text-primary no-underline"
            >
              {link.label}
            </a>
          ))}
        </nav>

        <div className="hidden items-center gap-3 md:flex">
          {secondaryAction ? <ActionButton action={secondaryAction} /> : null}
          <ActionButton action={primaryAction} />
        </div>

        <button
          type="button"
          className="border-0 bg-transparent p-1 text-text-secondary transition-colors duration-200 hover:text-text-primary md:hidden"
          onClick={() => setMenuOpen((value) => !value)}
          aria-label="Navigation umschalten"
        >
          {menuOpen ? <X size={22} /> : <Menu size={22} />}
        </button>
      </div>

      {menuOpen ? (
        <div className="glass border-t border-border md:hidden">
          <div className="mx-auto flex max-w-7xl flex-col gap-2 px-6 py-4">
            {navLinks.map((link) => (
              <a
                key={link.href}
                href={link.href}
                className="py-2 text-sm font-medium text-text-secondary no-underline transition-colors duration-200 hover:text-text-primary"
                onClick={() => setMenuOpen(false)}
              >
                {link.label}
              </a>
            ))}
            <div className="mt-3 flex flex-col gap-2 border-t border-border pt-3">
              {secondaryAction ? (
                <ActionButton
                  action={{ ...secondaryAction, onClick: () => setMenuOpen(false) }}
                />
              ) : null}
              <ActionButton
                action={{ ...primaryAction, onClick: () => setMenuOpen(false) }}
              />
            </div>
          </div>
        </div>
      ) : null}
    </header>
  );
}
