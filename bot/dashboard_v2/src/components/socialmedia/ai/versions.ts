import type { AiVideoVersion } from './api';

export function chooseUsableVersion(versions: AiVideoVersion[], preferredId?: number): AiVideoVersion | null {
  const usable = versions.filter((version) => version.file_url !== null && version.status !== 'failed' && version.status !== 'error');
  return usable.find((version) => version.id === preferredId) ?? [...usable].sort((a, b) => b.version_no - a.version_no)[0] ?? null;
}
