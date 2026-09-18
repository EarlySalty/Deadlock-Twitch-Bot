import type {
  AdManagerHistoryEntry,
  AdManagerPlan,
} from '@/api/adManager';

const ENTRY_TEXTE: Record<string, string> = {
  in_queue: 'Werbung in der Queue gestartet',
  match_start_window: 'Werbung zu Matchbeginn gestartet',
  post_match_quiet: 'Werbung nach dem Match gestartet',
  quiet_chat: 'Werbung in ruhiger Chat-Phase gestartet',
  fallback_least_bad: 'Werbung im ruhigsten Moment gestartet',
  pulled_forward: 'Geplante Werbung vorgezogen',
  twitch_plan_active: 'Twitch-Werbeplan aktiv, Fenster wird gesucht',
  in_match: 'Verschoben, dein Match läuft',
  post_match_wait: 'Verschoben, kurze Ruhe nach dem Match',
  post_match_chat_active: 'Verschoben, der Chat ist nach dem Match aktiv',
  recent_first_chatter: 'Verschoben, neuer Zuschauer im Chat',
  startup_protection: 'Verschoben, dein Stream läuft erst kurz',
  cooldown: 'Verschoben, Mindestabstand noch nicht erreicht',
  budget_reached: 'Werbebudget dieser Stunde erreicht',
  no_snoozes: 'Keine Twitch-Pausen mehr verfügbar',
  twitch_ad_moved: 'Twitch-Werbung in ein Fenster verschoben',
  disabled: 'Werbemanager ist aus',
  offline: 'Stream ist offline',
};

export function beschreibeEintrag(entry: AdManagerHistoryEntry): string {
  if (entry.reason === 'recent_raid') {
    return entry.detail ? `Verschoben, Raid von ${entry.detail}` : 'Verschoben, frischer Raid';
  }
  const text = ENTRY_TEXTE[entry.reason];
  if (text) return text;
  if (entry.decision === 'commercial') return 'Werbung gestartet';
  if (entry.decision === 'none') return 'Keine Aktion';
  return 'Verschoben';
}

const GEGENWART_TEXTE: Record<string, string> = {
  in_queue: 'Gerade läuft ein Werbeblock.',
  match_start_window: 'Gerade läuft ein Werbeblock.',
  post_match_quiet: 'Gerade läuft ein Werbeblock.',
  quiet_chat: 'Gerade läuft ein Werbeblock.',
  fallback_least_bad: 'Gerade läuft ein Werbeblock.',
  pulled_forward: 'Geplante Werbung wurde in ein gutes Fenster vorgezogen.',
  twitch_plan_active: 'Dein Twitch-Werbeplan ist aktiv, der Bot sucht gute Fenster.',
  in_match: 'Dein Match läuft, die Werbung wartet.',
  post_match_wait: 'Kurze Ruhe nach dem Match.',
  post_match_chat_active: 'Der Chat ist nach dem Match aktiv, die Werbung wartet.',
  recent_raid: 'Frischer Raid, die Werbung wartet.',
  recent_first_chatter: 'Neuer Zuschauer im Chat, die Werbung wartet.',
  startup_protection: 'Dein Stream läuft erst kurz, noch keine Werbung.',
  cooldown: 'Der Mindestabstand läuft noch.',
  budget_reached: 'Das Werbebudget dieser Stunde ist erfüllt.',
  twitch_ad_moved: 'Twitch-Werbung wurde in ein Fenster geschoben.',
  no_snoozes: 'Keine Twitch-Pausen mehr verfügbar.',
};

export function statusSatz(input: {
  enabled: boolean;
  isLive: boolean;
  currentReason: string | null;
  nextBlockLabel: string | null;
}): string {
  if (!input.enabled) return 'Aus. Der Bot schaut nur zu.';
  if (!input.isLive) return 'An. Sobald dein Stream läuft, kümmert sich der Bot um die Werbung.';
  const teile = ['Läuft.'];
  if (input.nextBlockLabel) teile.push(`Nächster Block ${input.nextBlockLabel}.`);
  const gegenwart = input.currentReason ? GEGENWART_TEXTE[input.currentReason] : undefined;
  if (gegenwart) teile.push(gegenwart);
  if (teile.length === 1) teile.push('Der Bot verteilt die Werbung über deinen Stream.');
  return teile.join(' ');
}

export function budgetVorschau(minutes: number, plan: AdManagerPlan | null): string {
  const blockSeconds = plan && plan.blockSeconds > 0 ? plan.blockSeconds : 30;
  const blocks = plan && plan.blocksPerHour > 0
    ? plan.blocksPerHour
    : Math.round((minutes * 60) / blockSeconds);
  if (blocks <= 0) return 'Noch kein Budget gesetzt.';
  const intervalMin = Math.max(1, Math.round(60 / blocks));
  return `${blocks} Blöcke à ${blockSeconds} Sek., etwa alle ${intervalMin} Min.`;
}
