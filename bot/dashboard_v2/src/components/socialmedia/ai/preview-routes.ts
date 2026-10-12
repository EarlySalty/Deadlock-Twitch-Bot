export * from '../../../preview/routes';
import { isPreviewLocalhost as genericPreview } from '../../../preview/routes';

export function isPreviewLocalhost(): boolean {
  return window.location.pathname !== '/src/components/socialmedia/ai/preview.html' && genericPreview();
}
