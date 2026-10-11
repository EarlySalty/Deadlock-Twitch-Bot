import { NavLink } from 'react-router';
import { PageHeader } from '@/components/layout/PageHeader';
import { ApiError } from '@/api/client';
import type { ReactNode } from 'react';
import './socialAi.css';

export function SocialAiLayout({ children }: { children: ReactNode }) {
  return <section className="social-ai space-y-5">
    <PageHeader title="KI-Videobearbeitung" description="Globale Lernschritte prüfen und den Verbrauch der Videobearbeitung begrenzen." />
    <nav className="flex flex-wrap gap-2" aria-label="KI-Verwaltung">
      {[
        ['/content/social-ai/skills', 'Skills & Vorschläge'],
        ['/content/social-ai/usage', 'Verbrauch & Kosten'],
        ['/content/social-ai/limits', 'Verarbeitungsgrenzen'],
      ].map(([to, label]) => <NavLink key={to} to={to} className={({ isActive }) =>
        `admin-button admin-button-secondary ${isActive ? 'social-ai-active' : ''}`}>
        {label}
      </NavLink>)}
    </nav>
    {children}
  </section>;
}

export function actionError(error: unknown): string {
  if (error instanceof ApiError && (error.status === 401 || error.status === 403)) {
    return 'Deine Admin-Berechtigung konnte nicht bestätigt werden. Lade die Seite neu und melde dich gegebenenfalls erneut an.';
  }
  if (error instanceof ApiError && error.status === 409) {
    return 'Der Stand wurde inzwischen geändert. Lade ihn neu und prüfe deine Änderung erneut.';
  }
  return 'Der aktuelle Stand konnte nicht bestätigt werden. Lade ihn neu, bevor du die Aktion erneut ausführst.';
}
