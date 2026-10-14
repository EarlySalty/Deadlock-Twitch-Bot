import { useId } from 'react';
import { Info } from 'lucide-react';

export function HintChip({ hint }: { hint: string }) {
  const id = useId();
  return <span className="studio-hint"><button type="button" className="studio-hint-button" aria-label={hint} aria-describedby={id}><Info size={14} aria-hidden /></button><span className="studio-hint-tooltip" id={id} role="tooltip">{hint}</span></span>;
}
