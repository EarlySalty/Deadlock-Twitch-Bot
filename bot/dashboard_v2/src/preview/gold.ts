import { isPreviewModeEnabled } from './routes';
import './gold.css';

const GOLD_VARIANTS = new Set(['polished', 'champagne', 'antique']);

export function applyGoldPreview() {
  if (!isPreviewModeEnabled()) return;
  const variant = new URLSearchParams(window.location.search).get('gold') ?? 'polished';
  if (variant === 'original') return;
  document.documentElement.dataset.gold = GOLD_VARIANTS.has(variant) ? variant : 'polished';
}
