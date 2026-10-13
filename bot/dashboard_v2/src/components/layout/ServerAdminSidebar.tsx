import { motion } from 'framer-motion';
import { Rise } from '@/motion/Rise';
import { SidebarLink, type SidebarNavItem } from '@/components/layout/SidebarLink';
import { SERVER_ADMIN_LOGOUT_URL, type GuildSettings, type ServerAdminSection } from '@/api/serverAdmin';
import { useLanguage } from '@/context/LanguageContext';
import { LANGUAGES, LANGUAGE_LABELS } from '@/i18n/dictionary';
import { Globe2, Hash, LayoutDashboard, LogOut, Swords, Users } from 'lucide-react';

export function ServerAdminLanguageSwitch() {
  const { language, setLanguage, t } = useLanguage();
  return (
    <div className="space-y-2">
      <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-text-secondary">
        {t('Dashboard-Sprache')}
      </div>
      <div
        className="inline-flex items-center rounded-xl border border-border bg-background/70 p-1.5"
        role="group"
        aria-label={t('Dashboard-Sprache')}
      >
        {LANGUAGES.map((option) => (
          <button
            key={option}
            type="button"
            lang={option}
            aria-label={LANGUAGE_LABELS[option]}
            aria-pressed={language === option}
            title={LANGUAGE_LABELS[option]}
            onClick={() => setLanguage(option)}
            className={`rounded-lg px-3 py-1.5 text-sm font-semibold transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary ${
              language === option
                ? 'gradient-accent text-on-gold'
                : 'text-text-secondary hover:text-white'
            }`}
          >
            {option.toUpperCase()}
          </button>
        ))}
      </div>
    </div>
  );
}

export function ServerAdminSidebar({
  displayName,
  guilds,
  activeGuildId,
  onGuildChange,
  activeSection,
}: {
  displayName: string;
  guilds: GuildSettings[];
  activeGuildId: string | null;
  onGuildChange: (guildId: string) => void;
  activeSection: ServerAdminSection;
}) {
  const { t } = useLanguage();
  const navItems: SidebarNavItem[] = [
    { href: '#overview', label: t('Server-Übersicht'), icon: LayoutDashboard, active: activeSection === 'overview' },
    { href: '#streamers', label: t('Streamer'), icon: Users, active: activeSection === 'streamers' },
    { href: '#channels', label: t('Kanäle & Rollen'), icon: Hash, active: activeSection === 'channels' },
    { href: '#language', label: t('Sprache & Einladung'), icon: Globe2, active: activeSection === 'language' },
    { href: '#raids', label: t('Raids'), icon: Swords, active: activeSection === 'raids' },
  ];

  return (
    <Rise as="aside" className="min-w-0 self-start lg:sticky lg:top-5">
      <div className="panel-card card-glow space-y-4 rounded-2xl p-4">
        <div className="flex items-center gap-3">
          <div className="gradient-accent sidebar-avatar-glow flex h-10 w-10 shrink-0 items-center justify-center rounded-full text-sm font-bold">
            {displayName?.[0]?.toUpperCase() ?? '?'}
          </div>
          <div className="min-w-0">
            <div className="truncate text-sm font-semibold text-white">{displayName}</div>
            <div className="mt-1 inline-flex max-w-full items-center rounded-full border border-accent/30 bg-accent/10 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-[0.18em] text-accent">
              {t('Server-Admin')}
            </div>
          </div>
        </div>

        {guilds.length > 1 ? (
          <div className="space-y-2">
            <label
              htmlFor="server-admin-guild"
              className="text-[11px] font-semibold uppercase tracking-[0.18em] text-text-secondary"
            >
              {t('Server')}
            </label>
            <select
              id="server-admin-guild"
              value={activeGuildId ?? ''}
              onChange={(event) => onGuildChange(event.target.value)}
              className="w-full rounded-xl border border-border bg-background/60 px-3 py-2 text-sm text-white"
            >
              {guilds.map((guild) => (
                <option key={guild.guildId} value={guild.guildId}>
                  {guild.name || guild.guildId}
                </option>
              ))}
            </select>
          </div>
        ) : null}

        <ServerAdminLanguageSwitch />
        <div className="border-t border-border" />

        <div className="space-y-2">
          <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-text-secondary">
            {t('Server verwalten')}
          </div>
          <nav className="flex gap-2 overflow-x-auto pb-1 lg:block lg:space-y-1 lg:overflow-visible lg:pb-0">
            {navItems.map((item, index) => (
              <motion.div
                key={item.href}
                initial={{ opacity: 0, x: -6 }}
                animate={{ opacity: 1, x: 0 }}
                transition={{ duration: 0.22, delay: Math.min(0.05 + index * 0.04, 0.24) }}
              >
                <SidebarLink href={item.href} icon={item.icon} label={item.label} active={item.active} />
              </motion.div>
            ))}
          </nav>
        </div>

        <div className="border-t border-border" />
        <div className="space-y-2">
          <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-text-secondary">
            {t('Konto')}
          </div>
          <a
            href={SERVER_ADMIN_LOGOUT_URL}
            className="flex items-center gap-2 rounded-xl border border-border bg-background/60 px-3 py-2 text-sm font-medium text-text-secondary transition-colors hover:border-border-hover hover:text-white"
          >
            <LogOut className="h-4 w-4" />
            {t('Abmelden')}
          </a>
        </div>
      </div>
    </Rise>
  );
}
