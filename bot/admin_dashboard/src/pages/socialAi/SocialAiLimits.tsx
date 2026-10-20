import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { RefreshCw } from 'lucide-react';
import { fetchLimits, saveGlobalControl, savePlanLimit, saveUserLimit, socialAiKeys } from '@/api/socialAi';
import type { GlobalControlChange, PlanLimitChange, ProcessingLimits, UserLimitChange } from '@/api/socialAiTypes';
import { formatDate, formatMicroUsd, GLOBAL_CAP_MICRO_USD, parseMicroUsd, usdInput } from '@/utils/socialAi';
import { ExternalAiConsent } from './ExternalAiConsent';
import { actionError, QueryState } from './SocialAiLayout';

type LimitChange = { kind: 'global'; body: GlobalControlChange } | { kind: 'user'; body: UserLimitChange } | { kind: 'plan'; body: PlanLimitChange };

function parseCount(value: string): number | null {
  return /^\d+$/.test(value) && Number.isSafeInteger(Number(value)) ? Number(value) : null;
}

function LimitForms({ data, blocked, saving, save }: { data: ProcessingLimits; blocked: boolean; saving: boolean; save: (change: LimitChange) => void }) {
  const [paused, setPaused] = useState(data.global.paused);
  const [globalCap, setGlobalCap] = useState(usdInput(data.global.monthly_cap_micro_usd));
  const [globalReason, setGlobalReason] = useState('');
  const [userId, setUserId] = useState('');
  const [userPaused, setUserPaused] = useState(false);
  const [userCap, setUserCap] = useState('');
  const [userJobs, setUserJobs] = useState('');
  const [userRevisions, setUserRevisions] = useState('');
  const [userReason, setUserReason] = useState('');
  const [planId, setPlanId] = useState(data.plans[0]?.plan_id ?? '');
  const initialPlan = data.plans[0];
  const [planCap, setPlanCap] = useState(initialPlan ? usdInput(initialPlan.monthly_cap_micro_usd) : '');
  const [planJobs, setPlanJobs] = useState(initialPlan?.monthly_ai_jobs?.toString() ?? '');
  const [planRevisions, setPlanRevisions] = useState(initialPlan?.monthly_revisions?.toString() ?? '');
  const [planReason, setPlanReason] = useState('');
  const user = data.users.find(user => user.twitch_user_id === userId.trim());
  const plan = data.plans.find(plan => plan.plan_id === planId);
  const globalAmount = globalCap.trim() === '' ? null : parseMicroUsd(globalCap);
  const userAmount = userCap.trim() === '' ? null : parseMicroUsd(userCap);
  const userJobCount = userJobs.trim() === '' ? null : parseCount(userJobs);
  const userRevisionCount = userRevisions.trim() === '' ? null : parseCount(userRevisions);
  const validUserCounts = (userJobs.trim() === '' || userJobCount !== null) && (userRevisions.trim() === '' || userRevisionCount !== null);
  const globalChanged = paused !== data.global.paused || globalAmount !== data.global.monthly_cap_micro_usd;
  const planAmount = planCap.trim() === '' ? null : parseMicroUsd(planCap);
  const validMoney = (input: string, amount: number | null) => input.trim() === '' || (amount !== null && amount <= GLOBAL_CAP_MICRO_USD);
  const userBlocked = blocked || !data.enforcement_available;
  const planBlocked = blocked || !data.plan_configuration_available;
  const userChanged = !user || user.paused !== userPaused || user.monthly_cap_micro_usd !== userAmount || user.monthly_ai_jobs !== userJobCount || user.monthly_revisions !== userRevisionCount;
  const jobs = planJobs.trim() === '' ? null : parseCount(planJobs);
  const revisions = planRevisions.trim() === '' ? null : parseCount(planRevisions);
  const validCounts = (planJobs.trim() === '' || jobs !== null) && (planRevisions.trim() === '' || revisions !== null);
  const planChanged = !!plan && (plan.monthly_cap_micro_usd !== planAmount || plan.monthly_ai_jobs !== jobs || plan.monthly_revisions !== revisions);

  function selectUser(id: string) {
    const existing = data.users.find(user => user.twitch_user_id === id.trim());
    setUserId(id); setUserPaused(existing?.paused ?? false); setUserCap(existing ? usdInput(existing.monthly_cap_micro_usd) : ''); setUserJobs(existing?.monthly_ai_jobs?.toString() ?? ''); setUserRevisions(existing?.monthly_revisions?.toString() ?? ''); setUserReason('');
  }

  function selectPlan(id: string) {
    const existing = data.plans.find(plan => plan.plan_id === id);
    setPlanId(id); setPlanCap(existing ? usdInput(existing.monthly_cap_micro_usd) : ''); setPlanJobs(existing?.monthly_ai_jobs?.toString() ?? ''); setPlanRevisions(existing?.monthly_revisions?.toString() ?? ''); setPlanReason('');
  }

  return <div className="space-y-5">
    <form className="panel-card rounded-2xl p-5 space-y-4" onSubmit={event => {
      event.preventDefault();
      if (!userBlocked && globalReason.trim() && globalChanged && validMoney(globalCap, globalAmount)) save({ kind: 'global', body: { expected_revision: data.global.revision, paused, monthly_cap_micro_usd: globalAmount, reason: globalReason.trim() } });
    }}>
      <h3 className="text-lg font-semibold">Globale Verarbeitung</h3>
      <dl className="social-ai-facts"><div><dt>Unveränderliche gemeinsame Testgrenze</dt><dd>{formatMicroUsd(GLOBAL_CAP_MICRO_USD)} <span className="text-sm text-text-secondary">(20.000.000 Mikro-USD)</span></dd></div><div><dt>Gespeicherter Stand</dt><dd>{data.global.paused ? 'Pausiert' : 'Nicht pausiert'} · Revision {data.global.revision}</dd></div></dl>
      <p className="text-sm text-text-secondary">Die gemeinsame Grenze wird nicht durch Nutzer- oder Planregeln angehoben. Das zentrale Verbrauchsprotokoll bleibt maßgeblich.</p>
      {!data.enforcement_available && <p className="social-ai-notice">Die serverseitige Durchsetzung ist noch nicht angebunden. Pause und Nutzergrenzen sind deshalb gesperrt.</p>}
      <label className="flex items-center gap-3"><input type="checkbox" checked={paused} disabled={userBlocked} onChange={event => setPaused(event.target.checked)} /> Neue KI-Anfragen global pausieren</label>
      <label className="block">Globales Monatsbudget in USD<input className="admin-input mt-2" inputMode="decimal" value={globalCap} disabled={userBlocked} aria-invalid={!validMoney(globalCap, globalAmount)} aria-describedby="global-cap-hint" onChange={event => setGlobalCap(event.target.value)} /><span id="global-cap-hint" className="mt-2 block text-sm text-text-secondary">0 bis 20 USD. Leer: keine zusätzliche Monatsgrenze. Die gemeinsame Testgrenze bleibt bestehen.</span></label>
      <label className="block">Begründung für die globale Änderung<textarea className="admin-input mt-2" rows={2} maxLength={1000} value={globalReason} disabled={userBlocked} onChange={event => setGlobalReason(event.target.value)} /></label>
      <button type="submit" className="admin-button admin-button-primary" disabled={userBlocked || !globalReason.trim() || !globalChanged || !validMoney(globalCap, globalAmount)}>{saving ? 'Wird gespeichert …' : 'Globale Grenzen speichern'}</button>
      <p className="text-sm text-text-secondary">Stand vom {formatDate(data.global.updated_at)} · {data.global.updated_by}. {data.global.reason}</p>
    </form>
    <form className="panel-card rounded-2xl p-5 space-y-4" onSubmit={event => {
      event.preventDefault();
      if (!userBlocked && /^\d+$/.test(userId.trim()) && userReason.trim() && userChanged && validMoney(userCap, userAmount) && validUserCounts) save({ kind: 'user', body: { twitch_user_id: userId.trim(), expected_revision: user?.revision ?? 0, paused: userPaused, monthly_cap_micro_usd: userAmount, monthly_ai_jobs: userJobCount, monthly_revisions: userRevisionCount, reason: userReason.trim() } });
    }}>
      <h3 className="text-lg font-semibold">Individuelle Nutzergrenzen</h3>
      <p className="text-sm text-text-secondary">Nutzergrenzen gelten zusätzlich zur gemeinsamen Grenze. Das Monatsbudget zählt den Verbrauch im aktuellen UTC-Monat und ungeklärte Reservierungen aus früheren Monaten. Ein leeres Budget entfernt die zusätzliche Monatsgrenze; 0 USD sperrt neue Anfragen. Bereits gestartete Anfragen werden nicht abgebrochen.</p>
      <div className="social-ai-form-grid">
        <label>Twitch-Nutzer-ID<input className="admin-input mt-2" inputMode="numeric" maxLength={32} value={userId} disabled={userBlocked} onChange={event => selectUser(event.target.value)} /></label>
        <label>Monatsbudget in USD<input className="admin-input mt-2" inputMode="decimal" value={userCap} disabled={userBlocked} aria-invalid={!validMoney(userCap, userAmount)} aria-describedby="user-cap-hint" onChange={event => setUserCap(event.target.value)} /><span id="user-cap-hint" className="mt-2 block text-sm text-text-secondary">0 bis 20 USD, bis zu sechs Nachkommastellen. Leer: keine zusätzliche Monatsgrenze.</span></label>
        <label>Erstproduktionen pro Monat<input className="admin-input mt-2" inputMode="numeric" value={userJobs} disabled={userBlocked} aria-invalid={userJobs.trim() !== '' && userJobCount === null} onChange={event => setUserJobs(event.target.value)} /></label>
        <label>Revisionen pro Monat<input className="admin-input mt-2" inputMode="numeric" value={userRevisions} disabled={userBlocked} aria-invalid={userRevisions.trim() !== '' && userRevisionCount === null} onChange={event => setUserRevisions(event.target.value)} /></label>
      </div>
      <p className="text-sm text-text-secondary">Anzahlgrenzen zählen neu begonnene Aufträge im UTC-Monat, keine einzelnen Analyseschritte. Leer: keine zusätzliche Anzahlgrenze. 0: keine neuen Aufträge dieser Art. Bereits begonnene Aufträge dürfen fortgesetzt werden.</p>
      <label className="flex items-center gap-3"><input type="checkbox" checked={userPaused} disabled={userBlocked} onChange={event => setUserPaused(event.target.checked)} /> Für diesen Nutzer pausieren</label>
      <label className="block">Begründung für die Nutzergrenze<textarea className="admin-input mt-2" rows={2} maxLength={1000} value={userReason} disabled={userBlocked} onChange={event => setUserReason(event.target.value)} /></label>
      {user && <p className="text-sm text-text-secondary">Nutzerrevision {user.revision}, Stand vom {formatDate(user.updated_at)}. {user.reason}</p>}
      <button type="submit" className="admin-button admin-button-primary" disabled={userBlocked || !/^\d+$/.test(userId.trim()) || !userReason.trim() || !userChanged || !validMoney(userCap, userAmount) || !validUserCounts}>Nutzergrenze speichern</button>
      {data.users.length === 0 ? <p className="text-text-secondary">Keine individuellen Nutzergrenzen gespeichert.</p> : <ul className="divide-y divide-border">{data.users.map(user => <li key={user.twitch_user_id} className="py-3 flex flex-wrap items-center justify-between gap-3">
        <div><p>Nutzer {user.twitch_user_id}: {user.paused ? 'pausiert' : 'nicht pausiert'}</p><p className="text-sm text-text-secondary">{user.monthly_cap_micro_usd === null ? 'Keine zusätzliche Monatsgrenze' : `${formatMicroUsd(user.monthly_cap_micro_usd)} pro Monat`} · Revision {user.revision}</p><p className="text-sm text-text-secondary">Erstproduktionen: {user.monthly_ai_jobs ?? 'Ohne Zusatzgrenze'} · Revisionen: {user.monthly_revisions ?? 'Ohne Zusatzgrenze'} pro Monat</p></div><button type="button" className="admin-button admin-button-secondary" disabled={userBlocked} onClick={() => selectUser(user.twitch_user_id)}>Nutzer {user.twitch_user_id} bearbeiten</button>
      </li>)}</ul>}
    </form>
    <form className="panel-card rounded-2xl p-5 space-y-4" onSubmit={event => {
      event.preventDefault();
      if (!planBlocked && plan && planReason.trim() && planChanged && validMoney(planCap, planAmount) && validCounts) save({ kind: 'plan', body: { plan_id: plan.plan_id, expected_revision: plan.revision, monthly_cap_micro_usd: planAmount, monthly_ai_jobs: jobs, monthly_revisions: revisions, reason: planReason.trim() } });
    }}>
      <h3 className="text-lg font-semibold">Künftige Planregeln: inaktiv</h3>
      <p className="text-sm text-text-secondary">Hier werden Regeln für vorhandene Abopläne vorbereitet, keine Preise festgelegt. Sie bleiben inaktiv, bis die bestehende Planauflösung und die serverseitige Durchsetzung angebunden sind.</p>
      {!data.plan_configuration_available && <p className="social-ai-notice">Planregeln können noch nicht gespeichert werden. Es wird keine aktive Begrenzung behauptet.</p>}
      {data.plans.length === 0 ? <p className="text-text-secondary">Noch keine vorhandenen Abopläne zur Konfiguration geliefert.</p> : <>
        <div className="social-ai-form-grid">
          <label>Vorhandener Aboplan<select className="admin-input mt-2" value={planId} disabled={planBlocked} onChange={event => selectPlan(event.target.value)}>{data.plans.map(plan => <option key={plan.plan_id} value={plan.plan_id}>{plan.name}</option>)}</select></label>
          <label>Vorgemerktes Monatsbudget in USD<input className="admin-input mt-2" inputMode="decimal" value={planCap} disabled={planBlocked} aria-invalid={!validMoney(planCap, planAmount)} onChange={event => setPlanCap(event.target.value)} /></label>
          <label>Vorgemerkte KI-Aufträge pro Monat<input className="admin-input mt-2" inputMode="numeric" value={planJobs} disabled={planBlocked} aria-invalid={planJobs.trim() !== '' && jobs === null} onChange={event => setPlanJobs(event.target.value)} /></label>
          <label>Vorgemerkte Revisionen pro Monat<input className="admin-input mt-2" inputMode="numeric" value={planRevisions} disabled={planBlocked} aria-invalid={planRevisions.trim() !== '' && revisions === null} onChange={event => setPlanRevisions(event.target.value)} /></label>
        </div>
        <p className="text-sm text-text-secondary">Leere Felder bedeuten keine zusätzliche Grenze. Die Werte sind Entwürfe und noch nicht wirksam.</p>
        <label className="block">Begründung für die Planregel<textarea className="admin-input mt-2" rows={2} maxLength={1000} value={planReason} disabled={planBlocked} onChange={event => setPlanReason(event.target.value)} /></label>
        {plan && <p className="text-sm text-text-secondary">Vorgemerkte Revision {plan.revision}. {plan.reason}</p>}
        <button type="submit" className="admin-button admin-button-secondary" disabled={planBlocked || !plan || !planReason.trim() || !planChanged || !validMoney(planCap, planAmount) || !validCounts}>Planregel inaktiv vormerken</button>
      </>}
    </form>
  </div>;
}

export function SocialAiLimitsPage() {
  const client = useQueryClient();
  const query = useQuery({ queryKey: socialAiKeys.limits, queryFn: ({ signal }) => fetchLimits(signal), retry: false, refetchOnWindowFocus: false });
  const [notice, setNotice] = useState('');
  const mutation = useMutation({ mutationFn: (change: LimitChange) => change.kind === 'global' ? saveGlobalControl(change.body) : change.kind === 'user' ? saveUserLimit(change.body) : savePlanLimit(change.body), retry: false,
    onSuccess: (data, change) => { client.setQueryData(socialAiKeys.limits, data); setNotice(change.kind === 'plan' ? 'Planregel vom Server gespeichert, weiterhin inaktiv.' : 'Geänderter Stand vom Server bestätigt.'); },
    onError: () => { void client.invalidateQueries({ queryKey: socialAiKeys.limits, refetchType: 'none' }); },
  });
  async function reload() {
    const result = await query.refetch();
    if (!result.error) { mutation.reset(); setNotice('Aktueller Serverstand geladen. Ungespeicherte Eingaben wurden verworfen.'); }
  }
  const data = query.data;
  return <div className="space-y-5">
    <div className="flex flex-wrap items-center justify-between gap-3"><h2 className="text-xl font-semibold">Verarbeitungsgrenzen</h2><button type="button" className="admin-button admin-button-secondary" disabled={query.isFetching || mutation.isPending} onClick={() => void reload()}><RefreshCw size={16} aria-hidden="true" /> Stand neu laden</button></div>
    <ExternalAiConsent />
    <p className="text-sm text-text-secondary">Ohne bestätigten Stand wird keine Änderung gesendet. Nach einem Konflikt oder unklaren Speicherergebnis muss der Stand neu geladen werden.</p>
    {mutation.error && <p role="alert" className="social-ai-notice">{actionError(mutation.error)}</p>}
    <p role="status" aria-live="polite" className="text-sm text-text-secondary">{notice}</p>
    <QueryState pending={query.isPending} error={query.error}>
      {data && <LimitForms key={`${query.dataUpdatedAt}:${JSON.stringify(data)}`} data={data} blocked={query.isFetching || !!query.error || mutation.isPending || mutation.isError} saving={mutation.isPending} save={change => { setNotice(''); mutation.mutate(change); }} />}
    </QueryState>
  </div>;
}
