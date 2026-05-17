import { useState, useEffect } from 'react';
import { Menu, X } from 'lucide-react';
import { useScrollSpy } from '@/hooks/useScrollSpy';
import { AFFILIATE_PROGRAM_PATH } from '@/data/sitePaths';

interface NavLink {
  label: string;
  id?: string;
  href?: string;
}

const NAV_LINKS: NavLink[] = [
  { label: 'Raids', id: 'raid' },
  { label: 'Moderation', id: 'moderation' },
  { label: 'Dashboard', id: 'dashboard' },
  { label: 'Features', id: 'features' },
  { label: 'Community', id: 'community' },
  { label: 'Vertriebler', href: AFFILIATE_PROGRAM_PATH },
];

const SECTION_IDS = NAV_LINKS.flatMap((link) => (link.id ? [link.id] : []));

function scrollToId(id: string) {
  document.getElementById(id)?.scrollIntoView({ behavior: 'smooth' });
}

export function Navbar() {
  const [glassy, setGlassy] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const activeId = useScrollSpy(SECTION_IDS);

  useEffect(() => {
    function handleScroll() {
      setGlassy(window.scrollY > 50);
    }
    window.addEventListener('scroll', handleScroll, { passive: true });
    return () => window.removeEventListener('scroll', handleScroll);
  }, []);

  // Close mobile menu on resize to desktop
  useEffect(() => {
    function handleResize() {
      if (window.innerWidth >= 768) setMenuOpen(false);
    }
    window.addEventListener('resize', handleResize);
    return () => window.removeEventListener('resize', handleResize);
  }, []);

  return (
    <header
      className={`fixed top-0 left-0 right-0 z-50 transition-all duration-300 ${glassy ? 'glass' : ''}`}
    >
      <div className="max-w-7xl mx-auto px-6 flex justify-between items-center h-16">
        {/* Logo */}
        <span className="font-display font-bold text-xl bg-gradient-to-r from-primary to-accent bg-clip-text text-transparent select-none">
          Deutsche Deadlock Community
        </span>

        {/* Center nav – desktop only */}
        <nav className="hidden md:flex items-center gap-8">
          {NAV_LINKS.map(({ label, id, href }) =>
            id ? (
              <button
                key={id}
                onClick={() => scrollToId(id)}
                className={`text-sm font-medium transition-colors duration-200 cursor-pointer bg-transparent border-none p-0 ${
                  activeId === id
                    ? 'text-text-primary'
                    : 'text-text-secondary hover:text-text-primary'
                }`}
              >
                {label}
              </button>
            ) : (
              <a
                key={href}
                href={href}
                className="text-sm font-medium text-text-secondary hover:text-text-primary transition-colors duration-200"
              >
                {label}
              </a>
            ),
          )}
        </nav>

        {/* Right actions – desktop only */}
        <div className="hidden md:flex items-center gap-3">
          <a
            href="https://deutsche-deadlock-community.de/demo/twitch/demo/"
            target="_blank"
            rel="noopener noreferrer"
            className="border border-border rounded-lg px-4 py-2 text-sm text-text-secondary hover:text-text-primary hover:border-border-hover transition-colors duration-200"
          >
            Demo ansehen
          </a>
          <button
            onClick={() => scrollToId('cta')}
            className="gradient-accent rounded-lg px-4 py-2 text-sm font-semibold text-white cursor-pointer border-none transition-opacity duration-200 hover:opacity-90"
          >
            Partner werden
          </button>
        </div>

        {/* Hamburger – mobile only */}
        <button
          className="md:hidden text-text-secondary hover:text-text-primary transition-colors duration-200 bg-transparent border-none p-1 cursor-pointer"
          onClick={() => setMenuOpen((prev) => !prev)}
          aria-label="Toggle menu"
        >
          {menuOpen ? <X size={22} /> : <Menu size={22} />}
        </button>
      </div>

      {/* Mobile dropdown */}
      {menuOpen && (
        <div className="md:hidden glass border-t border-border">
          <div className="max-w-7xl mx-auto px-6 py-4 flex flex-col gap-2">
            {NAV_LINKS.map(({ label, id, href }) =>
              id ? (
                <button
                  key={id}
                  onClick={() => {
                    scrollToId(id);
                    setMenuOpen(false);
                  }}
                  className={`text-sm font-medium text-left py-2 transition-colors duration-200 bg-transparent border-none cursor-pointer ${
                    activeId === id
                      ? 'text-text-primary'
                      : 'text-text-secondary hover:text-text-primary'
                  }`}
                >
                  {label}
                </button>
              ) : (
                <a
                  key={href}
                  href={href}
                  className="text-sm font-medium text-left py-2 text-text-secondary hover:text-text-primary transition-colors duration-200"
                  onClick={() => setMenuOpen(false)}
                >
                  {label}
                </a>
              ),
            )}
            <div className="flex flex-col gap-2 mt-3 pt-3 border-t border-border">
              <a
                href="https://deutsche-deadlock-community.de/demo/twitch/demo/"
                target="_blank"
                rel="noopener noreferrer"
                className="border border-border rounded-lg px-4 py-2 text-sm text-text-secondary hover:text-text-primary hover:border-border-hover transition-colors duration-200 text-center"
                onClick={() => setMenuOpen(false)}
              >
                Demo ansehen
              </a>
              <button
                onClick={() => {
                  scrollToId('cta');
                  setMenuOpen(false);
                }}
                className="gradient-accent rounded-lg px-4 py-2 text-sm font-semibold text-white cursor-pointer border-none transition-opacity duration-200 hover:opacity-90"
              >
                Partner werden
              </button>
            </div>
          </div>
        </div>
      )}
    </header>
  );
}
