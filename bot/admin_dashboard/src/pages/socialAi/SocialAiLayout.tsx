import { useEffect, useRef } from 'react';
import type { ReactNode } from 'react';
import { NavLink, Outlet } from 'react-router';
import { PageHeader } from '@/components/layout/PageHeader';
import { ConfirmDialog } from '@/components/shared/ConfirmDialog';
import { ApiError } from '@/api/client';
import './socialAi.css';

export function SocialAiLayout() {
  return <section className="social-ai space-y-5">
    <PageHeader title="KI-Videos verwalten" description="Globale Skill-Änderungen prüfen, Kosten einordnen und Verarbeitungsgrenzen verwalten." />
    <p className="social-ai-notice">Integrationsvorbereitung: Die Backend-Anbindung dieser Verwaltung steht noch aus. Ohne bestätigten Serverstand sind Änderungen gesperrt.</p>
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
  if (error instanceof ApiError && (error.status === 404 || error.status === 501)) {
    return 'Diese Backend-Funktion ist noch nicht verfügbar. Es wurde kein bestätigter Stand geladen.';
  }
  return 'Der aktuelle Stand konnte nicht bestätigt werden. Lade ihn neu, bevor du die Aktion erneut ausführst.';
}

export function QueryState({ pending, error, children }: { pending: boolean; error: unknown; children: ReactNode }) {
  if (pending) return <p role="status" className="social-ai-notice">Serverstand wird geladen …</p>;
  if (error) return <p role="alert" className="social-ai-notice">{actionError(error)}</p>;
  return children;
}

export function SocialAiConfirm({ title, description, busy, disabled, onCancel, onConfirm }: {
  title: string; description: string; busy: boolean; disabled: boolean; onCancel: () => void; onConfirm: () => void;
}) {
  const dialog = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const previous = document.activeElement;
    const overflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    dialog.current?.querySelector<HTMLButtonElement>('button')?.focus();
    return () => {
      document.body.style.overflow = overflow;
      if (previous instanceof HTMLElement) previous.focus();
    };
  }, []);
  return <div ref={dialog} role="dialog" aria-modal="true" aria-label={title} className="social-ai-dialog" onKeyDown={event => {
    if (event.key === 'Escape') {
      event.preventDefault();
      if (!busy) onCancel();
    }
    if (event.key === 'Tab') {
      const buttons = Array.from(dialog.current?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? []);
      const target = event.shiftKey ? buttons.at(-1) : buttons[0];
      const boundary = event.shiftKey ? buttons[0] : buttons.at(-1);
      if (document.activeElement === boundary && target) { event.preventDefault(); target.focus(); }
    }
  }}>
    <ConfirmDialog open title={title} description={description} confirmLabel="Zurücksetzen bestätigen" tone="danger" busy={busy} confirmDisabled={disabled} onCancel={onCancel} onConfirm={onConfirm} />
  </div>;
}
