import type {
  AdManagerHistoryEntry,
  AdManagerPlan,
  AdManagerPlanFit,
} from '@/api/adManager';
import { tr } from '../../i18n/dictionary';

function entryTexte(): Record<string, string> {
  return {
    in_queue: tr('Werbung in der Queue gestartet'),
    match_start_window: tr('Werbung zu Matchbeginn gestartet'),
    post_match_quiet: tr('Werbung nach dem Match gestartet'),
    quiet_chat: tr('Werbung in ruhiger Chat-Phase gestartet'),
    fallback_least_bad: tr('Werbung im ruhigsten Moment gestartet'),
    pulled_forward: tr('Geplante Werbung vorgezogen'),
    twitch_plan_active: tr('Twitch-Werbeplan aktiv, Fenster wird gesucht'),
    in_match: tr('Verschoben, dein Match läuft'),
    match_status_unknown: tr('Eigener Werbestart ausgesetzt, Matchstatus nicht bestätigt'),
    post_match_wait: tr('Verschoben, kurze Ruhe nach dem Match'),
    post_match_chat_active: tr('Verschoben, der Chat ist nach dem Match aktiv'),
    recent_first_chatter: tr('Verschoben, neuer Zuschauer im Chat'),
    startup_protection: tr('Verschoben, dein Stream läuft erst kurz'),
    cooldown: tr('Verschoben, Mindestabstand noch nicht erreicht'),
    budget_reached: tr('Werbebudget dieser Stunde erreicht'),
    no_snoozes: tr('Keine Twitch-Pausen mehr verfügbar'),
    twitch_ad_moved: tr('Twitch-Werbung in ein Fenster verschoben'),
    disabled: tr('Werbemanager ist aus'),
    offline: tr('Stream ist offline'),
  };
}

export function beschreibeEintrag(entry: AdManagerHistoryEntry): string {
  if (entry.reason === 'recent_raid') {
    return entry.detail ? tr('Verschoben, Raid von {name}', { name: entry.detail }) : tr('Verschoben, frischer Raid');
  }
  const text = entryTexte()[entry.reason];
  if (text) return text;
  if (entry.decision === 'commercial') return tr('Werbung gestartet');
  if (entry.decision === 'none') return tr('Keine Aktion');
  return tr('Verschoben');
}

function gegenwartTexte(): Record<string, string> {
  return {
    in_queue: tr('Gerade läuft ein Werbeblock.'),
    match_start_window: tr('Gerade läuft ein Werbeblock.'),
    post_match_quiet: tr('Gerade läuft ein Werbeblock.'),
    quiet_chat: tr('Gerade läuft ein Werbeblock.'),
    fallback_least_bad: tr('Gerade läuft ein Werbeblock.'),
    pulled_forward: tr('Geplante Werbung wurde in ein gutes Fenster vorgezogen.'),
    twitch_plan_active: tr('Dein Twitch-Werbeplan ist aktiv, der Bot sucht gute Fenster.'),
    in_match: tr('Dein Match läuft. Der Bot nutzt verfügbare Twitch-Pausen und startet keine eigene Werbung.'),
    match_status_unknown: tr('Kein frischer Matchstatus. Eigene Werbestarts warten; der Bot nutzt verfügbare Twitch-Pausen.'),
    post_match_wait: tr('Kurze Ruhe nach dem Match.'),
    post_match_chat_active: tr('Der Chat ist nach dem Match aktiv, die Werbung wartet.'),
    recent_raid: tr('Frischer Raid, die Werbung wartet.'),
    recent_first_chatter: tr('Neuer Zuschauer im Chat, die Werbung wartet.'),
    startup_protection: tr('Dein Stream läuft erst kurz, noch keine Werbung.'),
    cooldown: tr('Der Mindestabstand läuft noch.'),
    budget_reached: tr('Das Werbebudget dieser Stunde ist erfüllt.'),
    twitch_ad_moved: tr('Twitch-Werbung wurde in ein Fenster geschoben.'),
    no_snoozes: tr('Keine Twitch-Pausen mehr verfügbar.'),
  };
}

function fitTexte(): Partial<Record<AdManagerPlanFit, string>> {
  return {
    tight: tr('Dein Werbeplan ist dicht, der Bot schützt deine wichtigsten Momente.'),
    unprotectable: tr('Dein Werbeplan lässt sich nicht immer aus dem Match halten, der Bot nutzt jedes ruhige Fenster.'),
  };
}

export function statusSatz(input: {
  enabled: boolean;
  isLive: boolean;
  currentReason: string | null;
  nextBlockLabel: string | null;
  fit: AdManagerPlanFit | null;
}): string {
  if (!input.enabled) return tr('Aus. Der Bot schaut nur zu.');
  if (!input.isLive) return tr('An. Sobald dein Stream läuft, kümmert sich der Bot um die Werbung.');
  const teile = [tr('Läuft.')];
  if (input.nextBlockLabel) teile.push(tr('Nächster Block {time}.', { time: input.nextBlockLabel }));
  const gegenwart = input.currentReason ? gegenwartTexte()[input.currentReason] : undefined;
  if (gegenwart) teile.push(gegenwart);
  const fitText = input.fit ? fitTexte()[input.fit] : undefined;
  if (fitText) teile.push(fitText);
  if (teile.length === 1) teile.push(tr('Der Bot verteilt die Werbung über deinen Stream.'));
  return teile.join(' ');
}

export function budgetVorschau(minutes: number, plan: AdManagerPlan | null): string {
  const blockSeconds = plan && plan.blockSeconds > 0 ? plan.blockSeconds : 30;
  const blocks = plan && plan.blocksPerHour > 0
    ? plan.blocksPerHour
    : Math.round((minutes * 60) / blockSeconds);
  if (blocks <= 0) return tr('Noch kein Budget gesetzt.');
  const intervalMin = Math.max(1, Math.round(60 / blocks));
  return tr('{blocks} Blöcke à {seconds} Sek., etwa alle {minutes} Min.', {
    blocks,
    seconds: blockSeconds,
    minutes: intervalMin,
  });
}
