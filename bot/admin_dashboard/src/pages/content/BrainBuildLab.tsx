import { useEffect, useMemo, useState } from 'react';
import { Brain, Download, Loader2, RefreshCw } from 'lucide-react';
import { buildBrainPlan, fetchBrainCatalog } from '@/api/client';
import type { BrainBuild, BrainCatalog, BrainItem, BrainReport, BrainRequest, BrainScore } from '@/api/brainLab';
import { scoresForBuild } from '@/utils/brainLab.mjs';

const initial: BrainRequest = { hero: '', combat_window_seconds: 40, channel_uptime: 0.55, incoming_weapon_share: 0.5, incoming_pressure_dps: null };
const field = 'w-full rounded-xl border border-white/15 bg-background px-3 py-2 text-white';
const panel = 'rounded-2xl border border-white/10 bg-white/[0.03] p-5';
const number = (value: number | null | undefined, digits = 2) => value == null || !Number.isFinite(value) ? 'nicht belegt' : value.toLocaleString('de-DE', { maximumFractionDigits: digits });
const date = (value: number | null) => value ? new Date(value * 1000).toLocaleString('de-DE') : 'Zeitstempel fehlt';

function Purchase({ item, abilities }: { item: BrainItem; abilities: Map<number, string> }) {
  return <article className="rounded-xl border border-white/10 bg-white/[0.025] p-4">
    <div className="flex flex-wrap items-center justify-between gap-2"><h4 className="font-semibold text-white">{item.name}</h4><span className="text-xs text-text-secondary">{item.buy_phase} · Tier {item.tier} · Sicherheit: {item.confidence}</span></div>
    <p className="mt-2 whitespace-pre-wrap text-sm text-text-secondary">{item.why}</p>
    {item.imbue_target != null && <p className="mt-2 text-sm text-primary">Imbue → {abilities.get(item.imbue_target) ?? `nicht aufgelöste Fähigkeit ${item.imbue_target}`}</p>}
    {item.sell_priority != null && <p className="mt-1 text-sm text-warning">Verkaufskandidat · Priorität {item.sell_priority}</p>}
    {item.sources.length > 0 && <details className="mt-2 text-xs text-text-secondary"><summary className="cursor-pointer">Mechanik und Belege ({item.sources.length})</summary>{item.sources.map((e, i) => <p className="mt-2 whitespace-pre-wrap" key={i}><strong>{e.kind}:</strong> {e.detail}</p>)}</details>}
  </article>;
}

function Scores({ scores }: { scores: BrainScore[] }) {
  const [query, setQuery] = useState('');
  const filtered = useMemo(() => scores.filter(s => s.item.name.toLocaleLowerCase().includes(query.toLocaleLowerCase())).sort((a, b) => b.score.total - a.score.total || a.item.item_id - b.item.item_id), [scores, query]);
  return <section className={panel}>
    <h2 className="text-lg font-semibold text-white">Item-Mathematik dieser Variante</h2>
    <p className="mt-2 text-sm text-text-secondary">Alle geladenen Item-Kandidaten für genau diese Familie. Einzelitem-Scores sind keine Kaufreihenfolge: Der Planner bewertet zusätzlich Kombinationen, Upgrades, Bedingungen und Slots. Die Werte sind keine Prozent-Winrate.</p>
    <label className="mt-4 block text-sm">Item suchen<input value={query} onChange={e => setQuery(e.target.value)} className={`${field} mt-1`} placeholder="Zum Beispiel Spirit, Recharge …" /></label>
    {!scores.length && <p className="mt-3 text-warning">Keine Scores dieser Variante vorhanden. Es werden keine Werte einer anderen Familie eingesetzt.</p>}
    <div className="mt-4 overflow-x-auto"><table className="w-full min-w-[850px] text-left text-sm"><thead className="border-b border-white/15 text-text-secondary"><tr><th className="p-2">Item / Details</th><th className="p-2">Seelen</th><th className="p-2">Gesamt</th><th className="p-2">Kampf</th><th className="p-2">Je Slot</th><th className="p-2">Je Seele</th><th className="p-2">Bedingung</th><th className="p-2">Meta</th></tr></thead><tbody>
      {filtered.map(s => <tr key={s.item.item_id} className="border-b border-white/5 align-top"><td className="max-w-md p-2"><details><summary className="cursor-pointer font-medium text-white">{s.item.name} <span className="text-xs text-text-secondary">({s.item.slot})</span></summary><p className="mt-2 text-xs">ID {s.item.item_id} · {s.confidence} · Aktiv {number(s.score.active_value)} / Passiv {number(s.score.passive_value)} / Kaufbonus {number(s.score.purchase_bonus_value)}</p>{s.sources.map((e, i) => <p className="mt-2 whitespace-pre-wrap text-xs text-text-secondary" key={i}><strong>{e.kind}:</strong> {e.detail}</p>)}<pre className="mt-2 max-h-56 overflow-auto text-xs">{JSON.stringify(s.item.properties, null, 2)}</pre></details></td><td className="p-2">{number(s.item.cost, 0)}</td><td className="p-2">{number(s.score.total)}</td><td className="p-2">{number(s.score.combat_value)}</td><td className="p-2">{number(s.score.per_slot_value)}</td><td className="p-2">{number(s.score.per_soul_value, 5)}</td><td className="p-2">{number(s.score.condition_factor)}</td><td className="p-2">{number(s.score.meta_support)}</td></tr>)}
    </tbody></table></div>
  </section>;
}

export default function BrainBuildLab() {
  const [catalog, setCatalog] = useState<BrainCatalog | null>(null);
  const [input, setInput] = useState<BrainRequest>(initial);
  const [report, setReport] = useState<BrainReport | null>(null);
  const [variant, setVariant] = useState(0);
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [reload, setReload] = useState(0);
  useEffect(() => {
    let active = true; setLoading(true); setError('');
    fetchBrainCatalog().then(value => {
      if (!active) return; setCatalog(value);
      setInput(old => ({ ...old, hero: value.heroes.some(h => h.name === old.hero) ? old.hero : value.heroes.find(h => h.name === 'Warden')?.name ?? value.heroes[0]?.name ?? '' }));
    }).catch(err => { if (active) { setCatalog(null); setError(err instanceof Error ? err.message : 'Brain konnte nicht geladen werden.'); } }).finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [reload]);
  const plans: BrainBuild[] = report ? [report.build, ...report.build.variants] : [];
  const plan = plans[variant];
  const scores = report && plan ? scoresForBuild(report, plan) : [];
  const abilities = new Map(report?.hero.abilities.map(a => [a.ability_id, a.name]) ?? []);
  const change = <K extends keyof BrainRequest>(key: K, value: BrainRequest[K]) => { setInput(old => ({ ...old, [key]: value })); setReport(null); setVariant(0); };
  async function generate() {
    setBusy(true); setError(''); setReport(null); setVariant(0);
    try { setReport(await buildBrainPlan(input)); }
    catch (err) { setError(err instanceof Error ? err.message : 'Buildberechnung fehlgeschlagen.'); }
    finally { setBusy(false); }
  }
  function exportReport() {
    if (!report) return;
    const url = URL.createObjectURL(new Blob([JSON.stringify(report, null, 2)], { type: 'application/json' }));
    const a = document.createElement('a'); a.href = url; a.download = `brain-${report.hero.name.replace(/[^a-z0-9-]/gi, '-')}-${report.model_fingerprint.slice(0, 10)}.json`; a.click(); URL.revokeObjectURL(url);
  }
  return <div className="space-y-6">
    <header className="flex flex-wrap items-start justify-between gap-4"><div><p className="text-sm text-primary">Deadlock Brain · Rust Reasoner</p><h1 className="mt-1 flex items-center gap-3 text-3xl font-semibold text-white"><Brain className="h-8 w-8" />Build-Labor</h1><p className="mt-3 max-w-3xl text-text-secondary">Builds neu berechnen, Playstyles getrennt prüfen und Item-Zuordnungen nachvollziehen. Nur zum Testen – kein Steam-Publish und keine KI-Kosten.</p></div><button className="flex items-center gap-2 rounded-xl border border-white/15 px-4 py-2" disabled={busy || loading} onClick={() => { setReport(null); setReload(v => v + 1); }}><RefreshCw className="h-4 w-4" />Datenstatus laden</button></header>
    <div className="rounded-xl border border-warning/30 bg-warning/10 p-4 text-sm text-warning">Experimenteller Stand. Ein neuer Patch-Tag allein beweist keine aktuellen Itemdaten oder Nach-Patch-Matches. Quellenalter und offene Mechaniken bleiben sichtbar; ein erzeugter Build ist nicht automatisch fachlich freigegeben.</div>
    {error && <div role="alert" className="whitespace-pre-wrap rounded-xl border border-danger/40 bg-danger/10 p-4 text-danger">{error}</div>}
    {loading && <p role="status" className="flex items-center gap-2"><Loader2 className="h-4 w-4 animate-spin" />Brain-Datenstatus wird geladen.</p>}
    {catalog && <section className={panel}><p className="break-all text-sm text-text-secondary">Datenbank-Patch: <strong className="text-white">{catalog.patch_tag}</strong></p><div className="mt-2 flex flex-wrap gap-x-5 gap-y-1 text-xs text-text-secondary">{catalog.sources.map(s => <span key={`${s.source}-${s.entity_type}`}>{s.entity_type}: {date(s.fetched_at)}</span>)}</div>
      <form className="mt-5" onSubmit={e => { e.preventDefault(); void generate(); }}><fieldset disabled={busy} className="grid gap-4 md:grid-cols-2 xl:grid-cols-5">
        <label className="text-sm">Held<select required className={`${field} mt-1`} value={input.hero} onChange={e => change('hero', e.target.value)}>{catalog.heroes.map(h => <option key={h.id} value={h.name}>{h.name}</option>)}</select></label>
        <label className="text-sm">Kampffenster (Sekunden)<input required type="number" min={5} max={120} step={1} value={input.combat_window_seconds} onChange={e => change('combat_window_seconds', e.target.valueAsNumber)} className={`${field} mt-1`} /></label>
        <label className="text-sm">Kanal-Uptime (%)<input required type="number" min={0} max={100} step={1} value={Math.round(input.channel_uptime * 100)} onChange={e => change('channel_uptime', e.target.valueAsNumber / 100)} className={`${field} mt-1`} /></label>
        <label className="text-sm">Eingehender Waffenanteil (%)<input required type="number" min={0} max={100} step={1} value={Math.round(input.incoming_weapon_share * 100)} onChange={e => change('incoming_weapon_share', e.target.valueAsNumber / 100)} className={`${field} mt-1`} /></label>
        <label className="text-sm">Eingehende DPS (leer: Modell)<input type="number" min={0} max={2000} step={1} value={input.incoming_pressure_dps ?? ''} placeholder="Modellannahme" onChange={e => change('incoming_pressure_dps', e.target.value === '' ? null : e.target.valueAsNumber)} className={`${field} mt-1`} /></label>
      </fieldset><button disabled={busy || !input.hero} type="submit" className="mt-5 flex items-center gap-2 rounded-xl bg-primary px-5 py-3 font-semibold text-black disabled:opacity-50">{busy ? <Loader2 className="h-4 w-4 animate-spin" /> : <Brain className="h-4 w-4" />}{busy ? 'Rust-Reasoner berechnet …' : 'Builds neu berechnen'}</button></form>
    </section>}
    {report && plan && <>
      <section className={panel}><div className="flex flex-wrap items-center justify-between gap-4"><div><h2 className="text-xl font-semibold text-white">{report.hero.name}: {plans.length} berechnete Variante{plans.length === 1 ? '' : 'n'}</h2><p className="mt-1 text-sm text-text-secondary">Patch: {report.config.patch_tag} · {report.config.combat_window_seconds} s · Kanal {Math.round(report.config.channel_uptime * 100)} % · Sicherheit {plan.confidence}</p></div><button className="flex items-center gap-2 rounded-xl border border-white/15 px-4 py-2" onClick={exportReport}><Download className="h-4 w-4" />Prüfbericht als JSON</button></div>
        <label className="mt-5 block text-sm">Playstyle / Buildfamilie<select className={`${field} mt-1`} value={variant} onChange={e => setVariant(Number(e.target.value))}>{plans.map((p, i) => <option key={p.family?.id ?? i} value={i}>{p.family?.label ?? p.name}{i === 0 ? ' · dominant' : ''}</option>)}</select></label>
        {plan.family && <p className="mt-3 text-sm text-text-secondary">{plan.family.player_matches} historische und aktuelle Familien-Matches · {number(plan.family.post_patch_player_matches, 0)} nach Patch · {plan.family.distinct_players} Spieler · Kohärenz {number(plan.family.cohesion, 3)} · Skillorder-Support {number(plan.family.skill_order_support, 3)}</p>}
        <p className="mt-4 whitespace-pre-wrap text-sm text-text-secondary">{plan.rationale}</p>
        <details className="mt-4 text-sm" open><summary className="cursor-pointer font-semibold text-warning">Prüfhinweise und Grenzen ({report.warnings.length})</summary>{report.warnings.map((warning, i) => <p key={i} className="mt-2 text-warning/90">{warning}</p>)}</details>
      </section>
      <section className={panel}><h2 className="text-lg font-semibold text-white">Kaufplan · {plan.family?.label ?? plan.name}</h2><p className="mt-1 text-sm text-text-secondary">Reihenfolge inklusive früher Käufe, Upgrade-Komponenten und Verkaufskandidaten – nicht alle Einträge werden gleichzeitig getragen.</p><div className="mt-4 grid gap-3 lg:grid-cols-2">{plan.core.map((item, i) => <div key={`${item.item_id}-${i}`}><p className="mb-1 text-xs text-text-secondary">Kauf {i + 1}</p><Purchase item={item} abilities={abilities} /></div>)}</div></section>
      <section className={panel}><h2 className="text-lg font-semibold text-white">Skillorder dieser Variante</h2>{plan.ability_order.length ? <div className="mt-3 flex flex-wrap gap-2">{plan.ability_order.map((step, i) => <span className="rounded-lg border border-white/10 px-3 py-2 text-sm" key={i}>{i + 1}. {abilities.get(step.ability_id) ?? `Fähigkeit ${step.ability_id}`} (+{step.delta})</span>)}</div> : <p className="mt-2 text-warning">Keine belastbare Skillorder vorhanden. Es wird keine Folge erfunden.</p>}</section>
      {plan.situations.map((block, i) => <section className={panel} key={i}><h2 className="text-lg font-semibold text-white">{block.label} {block.optional ? '· optional' : ''}</h2><div className="mt-3 grid gap-3 lg:grid-cols-2">{block.items.map((item, j) => <Purchase key={`${item.item_id}-${j}`} item={item} abilities={abilities} />)}</div></section>)}
      <Scores key={plan.family?.id ?? 'default'} scores={scores} />
      <section className={panel}><h2 className="text-lg font-semibold text-white">Patch-Zuordnung und Quellen</h2><p className="mt-2 break-all font-mono text-xs text-text-secondary">Modell-Fingerprint: {report.model_fingerprint}</p>{report.sources.map(s => <p className="mt-2 text-sm text-text-secondary" key={s.source}>{s.source}: {date(s.oldest_fetched_at)} bis {date(s.newest_fetched_at)} · {s.fields_without_timestamp} Felder ohne Zeitstempel</p>)}<div className="mt-4 space-y-2">{report.patch_deltas.map((delta, i) => <details className="rounded-lg border border-white/10 p-3 text-sm" key={i}><summary className="cursor-pointer">{delta.application ? 'Angewandt' : 'Nicht zusätzlich angewandt'} · {delta.mechanic} · Betrag {number(delta.magnitude, 5)}</summary><p className="mt-2 whitespace-pre-wrap text-text-secondary">{delta.note}</p><pre className="mt-2 overflow-auto text-xs">{JSON.stringify({ target: delta.target, sign: delta.sign, application: delta.application }, null, 2)}</pre></details>)}</div></section>
    </>}
  </div>;
}
