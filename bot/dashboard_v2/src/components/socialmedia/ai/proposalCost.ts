import type { useT } from '@/context/LanguageContext';
import type { AiJobView } from './api';

export function proposalCost(proposal: NonNullable<AiJobView['revision_proposal']>, locale: string, t: ReturnType<typeof useT>) {
  if (proposal.estimated_provider_cost_usd === null) return t('Die zusätzlichen Anbieter-Kosten sind noch nicht zuverlässig schätzbar.');
  if (!proposal.reanalyze) return t('Dafür fallen keine weiteren KI-Kosten an, nur die Videoerstellung auf unserem Server.');
  return t('Geschätzte zusätzliche Anbieter-Kosten: etwa {cost} US-Dollar, gemessen am ersten Durchlauf dieses Clips.', { cost: proposal.estimated_provider_cost_usd.toLocaleString(locale, { maximumFractionDigits: 4 }) });
}
