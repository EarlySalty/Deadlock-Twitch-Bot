import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { History, RefreshCw } from 'lucide-react';
import { fetchSkills, rollbackSkill, socialAiKeys } from '@/api/socialAi';
import type { SkillHistory, SkillVersion } from '@/api/socialAiTypes';
import { ConfirmDialog } from '@/components/shared/ConfirmDialog';
import { EmptyState } from '@/components/shared/EmptyState';
import { formatCount, formatDate } from '@/utils/socialAi';
import { actionError, QueryState } from './SocialAiLayout';

const DIFF_LABELS = { added: 'Hinzugefügt', removed: 'Entfernt', context: 'Unverändert' } as const;
const DIFF_MARKS = { added: '+', removed: '−', context: ' ' } as const;
const PROPOSAL_STATUS = { proposed: 'Vorgeschlagen', reviewed: 'Manuell geprüft', dismissed: 'Verworfen' } as const;

function VersionDetails({ version }: { version: SkillVersion }) {
  return <div className="space-y-4 min-w-0">
    <h3 className="text-lg font-semibold">{version.previous_version ?? 'Erstversion'} → {version.version}</h3>
    {version.changed_at && <p className="text-sm text-text-secondary">Geändert am {formatDate(version.changed_at)}</p>}
    <p>{version.summary}</p>
    <dl className="social-ai-facts">
      <div><dt>Versionshash</dt><dd className="font-mono break-all">{version.content_hash ?? 'Nicht verfügbar'}</dd></div>
      <div><dt>Begründung</dt><dd>{version.reason || 'Nicht angegeben'}</dd></div>
      <div><dt>Qualitätsnachweis</dt><dd>{version.evidence?.summary || 'Kein Nachweis verfügbar'}{version.evidence && <span className="block text-sm text-text-secondary">Anonymisierte Beispiele: {formatCount(version.evidence.sample_count)}</span>}</dd></div>
    </dl>
    <h4 className="font-semibold">Textänderungen</h4>
    <p className="text-sm text-text-secondary">+ Hinzugefügt · − Entfernt · unveränderte Zeilen ohne Markierung</p>
    {version.diff.length === 0 ? <p className="text-text-secondary">Für diese Version liegt kein Textvergleich vor.</p> :
      <div className="social-ai-diff" role="region" aria-label={`Textvergleich für Version ${version.version}`} tabIndex={0}>
        {version.diff.map((line, index) => <div key={index} className={`social-ai-diff-${line.kind}`}><span aria-label={DIFF_LABELS[line.kind]}>{DIFF_MARKS[line.kind]}</span><pre>{line.text || ' '}</pre></div>)}
      </div>}
  </div>;
}

function SkillCard({ skill, disabled, onRollback }: { skill: SkillHistory; disabled: boolean; onRollback: (target: string, reason: string) => void }) {
  const [selected, setSelected] = useState(skill.versions[0]?.version ?? '');
  const [reason, setReason] = useState('');
  const version = skill.versions.find(version => version.version === selected);
  const target = version?.previous_version;
  return <article className="panel-card rounded-2xl p-5 space-y-4">
    <div className="flex flex-wrap items-start justify-between gap-3">
      <div><h2 className="text-xl font-semibold">{skill.name}</h2><p className="text-sm text-text-secondary break-all">{skill.skill_id} · Aktive Version: {skill.current_version}</p></div>
      <label className="social-ai-version-label">Änderung auswählen<select className="admin-input mt-1" value={selected} disabled={skill.versions.length === 0} onChange={event => { setSelected(event.target.value); setReason(''); }}>
        {skill.versions.map(version => <option key={version.version} value={version.version}>{version.previous_version ?? 'Erstversion'} → {version.version}{version.version === skill.current_version ? ' (aktuell)' : ''}</option>)}
      </select></label>
    </div>
    {version ? <VersionDetails version={version} /> : <p className="text-text-secondary">Noch keine Versionshistorie verfügbar.</p>}
    {target && target !== skill.current_version && <div className="border-t border-border pt-4 space-y-3">
      <p className="text-sm text-text-secondary">Zurücksetzen betrifft nur zukünftige Bearbeitungen. Bereits erzeugte Videos bleiben unverändert.</p>
      <label className="block">Begründung für das Zurücksetzen<textarea className="admin-input mt-2" rows={2} maxLength={1000} value={reason} disabled={disabled} onChange={event => setReason(event.target.value)} /></label>
      <button type="button" className="admin-button admin-button-danger" disabled={disabled || !reason.trim()} onClick={() => onRollback(target, reason.trim())}>Auf Version {target} zurücksetzen</button>
    </div>}
  </article>;
}

type Confirmation = { skillId: string; name: string; expected: string; target: string; reason: string };

export function SocialAiSkillsPage() {
  const client = useQueryClient();
  const query = useQuery({ queryKey: socialAiKeys.skills, queryFn: ({ signal }) => fetchSkills(signal), retry: false, refetchOnWindowFocus: false });
  const [confirmation, setConfirmation] = useState<Confirmation | null>(null);
  const [notice, setNotice] = useState('');
  const mutation = useMutation({
    mutationFn: (change: Confirmation) => rollbackSkill(change.skillId, { expected_version: change.expected, target_version: change.target, reason: change.reason }),
    retry: false,
    onSuccess: data => { client.setQueryData(socialAiKeys.skills, data); setConfirmation(null); setNotice('Die Zielversion wurde vom Server bestätigt.'); },
    onError: () => { setConfirmation(null); void client.invalidateQueries({ queryKey: socialAiKeys.skills, refetchType: 'none' }); },
  });
  const blocked = query.isFetching || !!query.error || mutation.isPending || mutation.isError || query.data?.rollback_available !== true;
  const current = query.data?.skills.find(skill => skill.skill_id === confirmation?.skillId)?.current_version;
  const stale = confirmation !== null && current !== confirmation.expected;

  async function reload() {
    const result = await query.refetch();
    if (!result.error) { mutation.reset(); setConfirmation(null); setNotice('Aktueller Serverstand geladen.'); }
  }

  return <div className="space-y-5">
    <div className="flex flex-wrap items-center justify-between gap-3"><h2 className="text-xl font-semibold">Globale Skill-Historie</h2><button type="button" className="admin-button admin-button-secondary" disabled={query.isFetching || mutation.isPending} onClick={() => void reload()}><RefreshCw size={16} aria-hidden="true" /> Stand neu laden</button></div>
    <QueryState pending={query.isPending} error={query.error}>
      {query.data && <>
        {query.data.skills.length === 0 && <EmptyState icon={History} title="Noch keine globalen Skill-Änderungen" description="Hier erscheinen versionierte Änderungen, sobald es welche gibt." />}
        {query.data.skills.map(skill => <SkillCard key={`${skill.skill_id}:${skill.current_version}`} skill={skill} disabled={blocked} onRollback={(target, reason) => {
          mutation.reset(); setNotice(''); setConfirmation({ skillId: skill.skill_id, name: skill.name, expected: skill.current_version, target, reason });
        }} />)}
        <section className="panel-card rounded-2xl p-5 space-y-4">
          <h2 className="text-xl font-semibold">Technische Vorschläge</h2>
          <p className="text-sm text-text-secondary">Vorschläge für Code, Architektur oder Werkzeuge werden hier nur angezeigt. Diese Ansicht führt sie nicht aus und veröffentlicht keine Änderungen.</p>
          {query.data.proposals.length === 0 ? <p className="text-text-secondary">Keine technischen Vorschläge vorhanden.</p> : <ul className="divide-y divide-border">{query.data.proposals.map(proposal => <li key={proposal.id} className="py-4 space-y-2">
            <div className="flex flex-wrap justify-between gap-2"><h3 className="font-semibold">{proposal.title}</h3><span className="text-sm text-text-secondary">{PROPOSAL_STATUS[proposal.status]}</span></div>
            <p className="text-sm text-text-secondary">{formatDate(proposal.created_at)}</p><p>{proposal.summary}</p><p className="text-sm text-text-secondary">Begründung: {proposal.reason}</p><p className="text-sm text-text-secondary">Nachweis: {proposal.evidence || 'Nicht verfügbar'}</p>
          </li>)}</ul>}
        </section>
      </>}
    </QueryState>
    {mutation.error && <p role="alert" className="social-ai-notice">{actionError(mutation.error)}</p>}
    <p role="status" aria-live="polite" className="text-sm text-text-secondary">{notice}</p>
    <ConfirmDialog
      open={confirmation !== null}
      title="Skill-Version zurücksetzen?"
      description={confirmation ? `${confirmation.name}: aktive Version ${confirmation.expected}, Zielversion ${confirmation.target}. Begründung: ${confirmation.reason}${stale ? ' Der aktive Stand hat sich geändert. Bitte abbrechen und neu laden.' : ''}` : ''}
      confirmLabel="Zurücksetzen"
      tone="danger"
      busy={mutation.isPending}
      onCancel={() => setConfirmation(null)}
      onConfirm={() => { if (confirmation && !blocked && !stale) mutation.mutate(confirmation); }}
    />
  </div>;
}
