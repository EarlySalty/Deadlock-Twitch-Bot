import { ANALYTICS_EN } from './analytics';
import { BACKEND_EN } from './backend';
import { CHARTS_EN } from './charts';
import { CHAT_EN } from './chat';
import { COACHING_EN } from './coaching';
import { MONETIZATION_EN } from './monetization';
import { OVERLAY_EN } from './overlay';
import { SHELL_EN } from './shell';
import { SOCIAL_EN } from './social';
import { UPLINK_EN } from './uplink';
import { VERWALTUNG_EN } from './verwaltung';

export const EN_AREAS: Record<string, Record<string, string>> = {
  shell: SHELL_EN,
  social: SOCIAL_EN,
  analytics: ANALYTICS_EN,
  charts: CHARTS_EN,
  chat: CHAT_EN,
  coaching: COACHING_EN,
  verwaltung: VERWALTUNG_EN,
  overlay: OVERLAY_EN,
  uplink: UPLINK_EN,
  monetization: MONETIZATION_EN,
  backend: BACKEND_EN,
};

export const EN_AREA_ENTRIES: Record<string, string> = Object.assign({}, ...Object.values(EN_AREAS));
