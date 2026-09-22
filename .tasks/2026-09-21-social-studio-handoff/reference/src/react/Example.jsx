import React, { useState } from 'react';
import { DashboardShell, ClipQueueCard, AutoPilotSchedule, WorkspaceDialog } from './index.js';
import { DEFAULT_SCHEDULE, DEMO_CLIPS } from '../model.js';
import '../styles.css';
import '../../dist/tailwind.css';
/** Drop into an existing React entry point. Demo only: no network or persistence. */
export default function Example() {
    const [activeTab, setActiveTab] = useState('queue');
    const [clips, setClips] = useState(() => structuredClone(DEMO_CLIPS));
    const [plan, setPlan] = useState(() => structuredClone(DEFAULT_SCHEDULE));
    const [editor, setEditor] = useState(null);
    const [text, setText] = useState('');
    function openTranscript(clip) { setText(clip.transcript || ''); setEditor(clip); }
    async function approve(id, targets) { setClips(current => current.map(clip => clip.id === id ? { ...clip, targets, status: 'scheduled', scheduledLabel: 'Nächster freier Slot · Demo' } : clip)); }
    async function archive(id) { setClips(current => current.map(clip => clip.id === id ? { ...clip, status: 'archived' } : clip)); }
    return (<DashboardShell streamer="earlysalty" demo activeTab={activeTab} onTabChange={setActiveTab} title="Social Studio" description="React-Komponenten mit lokalen Beispieldaten.">
      {activeTab === 'queue' && <div className="space-y-3">{clips.filter(clip => clip.status === 'review').map(clip => <ClipQueueCard key={clip.id} clip={clip} streamer="earlysalty" availableTargets={['youtube', 'tiktok']} onApprove={approve} onArchive={archive} onEditTranscript={openTranscript}/>)}{!clips.some(clip => clip.status === 'review') && <p className="card p-8 text-center">Alle Beispielclips geprüft.</p>}</div>}
      {activeTab === 'autopilot' && <AutoPilotSchedule key="earlysalty" value={plan} onSave={async (next) => { setPlan(next); return next; }}/>}
      {activeTab === 'templates' && <p className="card p-6 text-sm text-text-secondary">Integrationspunkt: Hier euren bestehenden LayoutEditor einbinden. Der schematische Layout-Dialog ist in preview.html bedienbar.</p>}
      {activeTab === 'accounts' && <p className="card p-6 text-sm text-text-secondary">Integrationspunkt: Hier euren bestehenden Plattform-Status und OAuth-Flow einbinden. Die Offline-Vorschau zeigt die ausgearbeitete Kontenansicht.</p>}
      {editor && <WorkspaceDialog small title={editor.title} description="Transkript · Demo" onClose={() => setEditor(null)}><label className="text-sm">Transkript<textarea className="field mt-3 min-h-48" value={text} onChange={event => setText(event.target.value)}/></label><button className="btn btn-primary mt-4" onClick={() => { setClips(current => current.map(clip => clip.id === editor.id ? { ...clip, transcript: text } : clip)); setEditor(null); }}>In der Demo speichern</button></WorkspaceDialog>}
    </DashboardShell>);
}
