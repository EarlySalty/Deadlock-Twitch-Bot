export type HealthScoreMetricKey = 'growth' | 'retention' | 'engagement' | 'community';

export interface HealthScoreMetricExplanation {
  key: HealthScoreMetricKey;
  label: string;
  weight: number;
  summary: string;
  detail: string;
}

export const HEALTH_SCORE_METRICS: readonly HealthScoreMetricExplanation[] = [
  {
    key: 'growth',
    label: 'Wachstum',
    weight: 30,
    summary: 'Ø Viewer im Vergleich zu den 7 Tagen davor',
    detail:
      '50 Punkte bedeuten ungefähr gleich viele durchschnittliche Viewer wie in den 7 Tagen davor. Jede Veränderung um 1 Prozent verschiebt den Wert um etwa 1 Punkt, begrenzt auf 0 bis 100. Fehlen Vergleichsdaten, startet Wachstum neutral bei 50.',
  },
  {
    key: 'retention',
    label: 'Konstanz',
    weight: 25,
    summary: 'Aktive Streamtage in den letzten 7 Tagen',
    detail:
      'Jeder unterschiedliche Streamtag zählt 15 Punkte. Der Wert zeigt also Regelmäßigkeit und nicht, wie lange einzelne Viewer im Stream bleiben.',
  },
  {
    key: 'engagement',
    label: 'Chat-Aktivität',
    weight: 25,
    summary: 'Chat-Nachrichten relativ zu deinen Ø Viewern',
    detail:
      'Wenn Chatdaten vorliegen, rechnen wir Chat-Nachrichten geteilt durch Ø Viewer mal 2, begrenzt auf 100. Mehr aktive Unterhaltung pro Viewer erhöht den Wert; ohne Chat-Signal bleibt er neutral bei 50.',
  },
  {
    key: 'community',
    label: 'Stammcommunity',
    weight: 20,
    summary: 'Anteil wiederkehrender Chatter in den letzten 7 Tagen',
    detail:
      'Misst den Anteil wiederkehrender Chatter unter den erkannten Chattern. Bekannte Chat-Bots und dein eigener Account werden dabei herausgefiltert.',
  },
] as const;

export type HealthScoreBand = 'Ausbaufähig' | 'Solide' | 'Stark';

export function healthScoreBand(value: number): HealthScoreBand {
  const score = Math.max(0, Math.min(100, Number.isFinite(value) ? value : 0));
  if (score >= 70) return 'Stark';
  if (score >= 40) return 'Solide';
  return 'Ausbaufähig';
}

export function healthScoreBandRange(value: number): string {
  const band = healthScoreBand(value);
  if (band === 'Stark') return '70 bis 100';
  if (band === 'Solide') return '40 bis 69';
  return '0 bis 39';
}
