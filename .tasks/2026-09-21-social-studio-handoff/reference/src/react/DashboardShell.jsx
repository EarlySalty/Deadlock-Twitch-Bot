import React, { useId, useRef } from 'react';
import { Icon } from './Icon.jsx';
import { CommunityBrand } from './CommunityBrand.jsx';
export const STUDIO_TABS = [
    { id: 'queue', label: 'Pipeline', icon: 'layers' },
    { id: 'autopilot', label: 'Auto-Pilot & Zeitplan', icon: 'bolt' },
    { id: 'templates', label: 'Templates & Layouts', icon: 'crop' },
    { id: 'accounts', label: 'Konten & Einstellungen', icon: 'sliders' },
];
/** Controlled navigation. Keep the active tab in your router or parent state. */
export function DashboardShell({ streamer, activeTab, onTabChange, title, description, actions, children, demo = false, navigation = [
    { label: 'Übersicht', href: '/analyse', icon: 'chart' },
    { label: 'Social Studio', href: '/social-media-admin', icon: 'film', current: true },
    { label: 'Verwaltung', href: '/twitch/verwaltung', icon: 'sliders' },
], }) {
    const prefix = useId();
    const tabRefs = useRef([]);
    const selected = STUDIO_TABS.findIndex(tab => tab.id === activeTab);
    function moveTab(event, index) {
        const last = STUDIO_TABS.length - 1;
        const next = {
            ArrowRight: (index + 1) % STUDIO_TABS.length,
            ArrowLeft: (index + last) % STUDIO_TABS.length,
            Home: 0,
            End: last,
        }[event.key];
        if (next === undefined)
            return;
        event.preventDefault();
        onTabChange(STUDIO_TABS[next].id);
        tabRefs.current[next]?.focus();
    }
    return (<div className="studio-shell min-h-screen bg-bg text-text-primary">
      <a href={`#${prefix}-main`} className="skip-link">Zum Inhalt springen</a>
      <aside className="fixed inset-y-0 left-0 hidden w-[216px] flex-col border-r border-border bg-bg px-4 py-6 lg:flex">
        <a href="/social-media-admin" className="brand-lockup flex items-center gap-3 px-2 font-semibold" aria-label="Deutsche Deadlock Community · Social Studio">
          <CommunityBrand />
        </a>
        <p className="mb-3 mt-10 px-3 text-xs tracking-widest text-text-secondary">WORKSPACE</p>
        <nav aria-label="Globale Navigation" className="sidebar-nav space-y-1">
          {navigation.map(link => <a key={link.href} href={link.href} aria-current={link.current ? 'page' : undefined} className="flex items-center gap-3 rounded-lg px-3 py-3 text-sm text-text-secondary hover:bg-white/[0.04] hover:text-text-primary"><Icon name={link.icon}/>{link.label}</a>)}
        </nav>
        <div className="mt-auto border-t border-border pt-5">
          <p className="text-sm text-text-primary">{streamer}</p>
          <p className="mt-1 text-xs text-text-secondary">Streamer-Workspace</p>
          {demo && <p className="mt-4 text-xs text-accent">Demo · keine echten Uploads</p>}
        </div>
      </aside>
      <div className="lg:pl-[216px]">
        <header className="flex min-h-16 items-center justify-between gap-3 border-b border-border px-4 text-xs text-text-secondary sm:px-8">
          <span className="flex items-center gap-2.5"><CommunityBrand compact /><span><span className="hidden sm:inline">Workspace <span className="mx-2 text-text-secondary">/</span></span>Social Studio</span></span>
          <span>{demo ? 'DEMO · ' : ''}{streamer}</span>
        </header>
        <main id={`${prefix}-main`} className="mx-auto max-w-[1440px] px-4 py-7 sm:px-8 sm:py-9" tabIndex={-1}>
          <div className="mb-6 flex flex-wrap items-center justify-between gap-4">
            <div><h1 className="text-2xl font-semibold sm:text-[28px]">{title}</h1><p className="mt-2 text-sm leading-6 text-text-secondary">{description}</p></div>
            <div className="flex items-center gap-2">{actions}</div>
          </div>
          <div role="tablist" aria-label="Social-Studio-Bereiche" className="overflow-scroll mb-7 flex gap-7 overflow-x-auto border-b border-border">
            {STUDIO_TABS.map((tab, index) => (<button key={tab.id} ref={element => { tabRefs.current[index] = element; }} id={`${prefix}-tab-${tab.id}`} type="button" role="tab" aria-selected={selected === index} aria-controls={`${prefix}-panel-${tab.id}`} tabIndex={selected === index ? 0 : -1} onKeyDown={event => moveTab(event, index)} onClick={() => onTabChange(tab.id)} className="tab-button"><Icon name={tab.icon}/>{tab.label}</button>))}
          </div>
          <section id={`${prefix}-panel-${activeTab}`} role="tabpanel" aria-labelledby={`${prefix}-tab-${activeTab}`} className="min-w-0">{children}</section>
        </main>
      </div>
    </div>);
}
