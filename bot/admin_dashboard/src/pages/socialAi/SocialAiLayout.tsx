import type { ReactNode } from 'react';
import { NavLink, Outlet } from 'react-router';
import { PageHeader } from '@/components/layout/PageHeader';
import { ApiError } from '@/api/client';
import './socialAi.css';

export function SocialAiLayout() {
  return <section className="social-ai space-y-5">
    <PageHeader title="KI-Videos verwalten" description="Verbrauch einordnen, Verarbeitungsgrenzen verwalten und Skill-Versionen prüfen." />
    <nav className="flex flex-wrap gap-2" aria-label="KI-Verwaltung">
      {[
        ['/content/social-ai/usage', 'Verbrauch & Kosten'],
        ['/content/social-ai/limits', 'Verarbeitungsgrenzen'],
        ['/content/social-ai/skills', 'Skill-Versionen'],
      ].map(([to, label]) => <NavLink key={to} to={to} className={({ isActive }) =>
        `admin-button admin-button-secondary ${isActive ? 'social-ai-active' : ''}`}>
        {label}
      </NavLink>)}
    </nav>
    <Outlet />
  </section>;
}

export function actionError(error: unknown): string {
  if (error instanceof ApiError && (error.status === 401 || error.status === 403)) {
    return 'Deine Admin-Berechtigung konnte nicht bestätigt werden. Lade die Seite neu und melde dich gegebenenfalls erneut an.';
  }
  if (error instanceof ApiError && error.status === 409) {
    return 'Der Stand wurde inzwischen geändert. Lade ihn neu und prüfe deine Änderung erneut.';
  }
  if (error instanceof ApiError && error.status === 404) {
    return 'Der angefragte Eintrag wurde nicht gefunden. Prüfe die Nutzer-ID und lade den Stand neu.';
  }
  return 'Der aktuelle Stand konnte nicht bestätigt werden. Lade ihn neu, bevor du die Aktion erneut ausführst.';
}

export function QueryState({ pending, error, children }: { pending: boolean; error: unknown; children: ReactNode }) {
  if (pending) return <p role="status" className="social-ai-notice">Serverstand wird geladen …</p>;
  if (error) return <p role="alert" className="social-ai-notice">{actionError(error)}</p>;
  return children;
}
