import { motion } from 'framer-motion';
import { Globe, Users, Clock, Heart, Activity, TrendingUp } from 'lucide-react';
import { PieChart, Pie, Cell, ResponsiveContainer, Tooltip } from 'recharts';
import {
  CHAT_AUDIENCE_TOOLTIP,
  getChattersCoverage,
  resolveChatPenetration,
  resolveMessagesPer100ViewerMinutes,
  resolveQualityMethod,
} from '@/utils/engagementKpi';

export interface AudienceDemographicsData {
  viewerTypes: { label: string; percentage: number }[];
  activityPattern: 'weekend-heavy' | 'weekday-focused' | 'balanced';
  primaryLanguage: string;
  languageConfidence: number;
  peakActivityHours: number[];
  peakHoursMethod?: string;
  chatPenetrationPct?: number | null;
  chatPenetrationReliable?: boolean;
  messagesPer100ViewerMinutes?: number | null;
  viewerMinutes?: number;
  legacyInteractionActivePerAvgViewer?: number | null;
  interactiveRate: number;
  interactionRateActivePerViewer?: number;
  interactionRateActivePerAvgViewer?: number | null;
  interactionRateReliable?: boolean;
  loyaltyScore: number;
  timezone?: string;
  dataQuality?: {
    confidence: 'very_low' | 'low' | 'medium' | 'high';
    method?: 'no_data' | 'low_coverage' | 'real_samples' | string;
    peakMethod?: 'no_data' | 'low_coverage' | 'real_samples' | string;
    sessions?: number;
    coverage?: number;
    sampleCount?: number;
    peakSessionCount?: number;
    peakSessionsWithActivity?: number;
    interactiveSampleCount?: number;
    interactionCoverage?: number;
    chattersCoverage?: number;
    passiveViewerSamples?: number;
  };
}

interface AudienceDemographicsProps {
  data: AudienceDemographicsData;
}

const VIEWER_COLORS = ['#10b981', '#34d399', '#8b5cf6', '#6ee7b7', '#a7f3d0'];

export function AudienceDemographics({ data }: AudienceDemographicsProps) {
  const activityLabels = {
    'weekend-heavy': 'Wochenend-fokussiert',
    'weekday-focused': 'Wochentags-fokussiert',
    'balanced': 'Ausgewogen',
  };

  const confidenceLabel =
    data.dataQuality?.confidence === 'very_low'
      ? 'sehr niedrig'
      : data.dataQuality?.confidence === 'low'
        ? 'niedrig'
        : data.dataQuality?.confidence === 'medium'
          ? 'mittel'
          : data.dataQuality?.confidence === 'high'
            ? 'hoch'
            : null;
  const chatMethod = resolveQualityMethod(
    data.dataQuality?.method,
    Boolean(data.chatPenetrationPct ?? data.interactiveRate ?? data.messagesPer100ViewerMinutes)
  );
  const peakMethod = resolveQualityMethod(
    data.dataQuality?.peakMethod ?? data.dataQuality?.method,
    (data.peakActivityHours ?? []).length > 0
  );
  const methodLabel =
    chatMethod === 'no_data'
      ? 'Keine Chat-Samples'
      : chatMethod === 'low_coverage'
        ? 'Low Coverage'
        : 'Echt-Samples';
  const normalizedPeakHours = Array.from(
    new Set(
      (data.peakActivityHours ?? [])
        .map((h) => Number(h))
        .filter((h) => Number.isFinite(h))
        .map((h) => Math.max(0, Math.min(23, Math.trunc(h))))
    )
  );
  const hasPeakData = peakMethod === 'real_samples' && normalizedPeakHours.length > 0;
  const penetration = resolveChatPenetration(data);
  const interactionReliable = penetration.reliable;
  const interactionCoveragePct = penetration.coverage * 100;
  const interactionRate =
    penetration.value ??
    (typeof data.interactiveRate === 'number' ? data.interactiveRate : null);
  const messagesPer100ViewerMinutes = resolveMessagesPer100ViewerMinutes(data);
  const chattersCoverage = getChattersCoverage(data.dataQuality);

  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      className="bg-card rounded-xl border border-border p-6"
    >
      <div className="flex items-center justify-between mb-6">
        <div className="flex items-center gap-3">
          <div className="w-10 h-10 rounded-lg bg-gradient-to-br from-primary/20 to-accent/20 flex items-center justify-center">
            <Globe className="w-5 h-5 text-primary" />
          </div>
          <div>
            <h3 className="text-lg font-bold text-white">Audience Demographics</h3>
            <p className="text-sm text-text-secondary">Reach- und Chat-Engagement-Mix</p>
          </div>
        </div>
        {data.dataQuality && confidenceLabel && (
          <span className="text-xs px-3 py-1 rounded-full border border-border text-text-secondary">
            Chat-KPI: {methodLabel} · Vertrauen: {confidenceLabel}
          </span>
        )}
      </div>
      <div className="mb-4 text-xs text-text-secondary">{CHAT_AUDIENCE_TOOLTIP}</div>
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        {/* Left Column - Charts */}
        <div className="space-y-6">
          {/* Viewer Types */}
          <div>
            <h4 className="text-sm font-medium text-text-secondary mb-3 flex items-center gap-2">
              <Users className="w-4 h-4" />
              Viewer-Typen
            </h4>
            <div className="flex items-center gap-4">
              <div className="w-32 h-32">
                <ResponsiveContainer width="100%" height="100%">
                  <PieChart>
                    <Pie
                      data={data.viewerTypes}
                      dataKey="percentage"
                      nameKey="label"
                      cx="50%"
                      cy="50%"
                      innerRadius={25}
                      outerRadius={50}
                      paddingAngle={2}
                    >
                      {data.viewerTypes.map((_, index) => (
                        <Cell key={index} fill={VIEWER_COLORS[index % VIEWER_COLORS.length]} />
                      ))}
                    </Pie>
                    <Tooltip
                      contentStyle={{
                        backgroundColor: '#1f2937',
                        border: '1px solid #374151',
                        borderRadius: '8px',
                      }}
                      formatter={(value?: number) => [`${(value ?? 0).toFixed(1)}%`, 'Anteil']}
                    />
                  </PieChart>
                </ResponsiveContainer>
              </div>
              <div className="flex-1 space-y-2">
                {data.viewerTypes.map((type, i) => (
                  <div key={type.label} className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <div
                        className="w-3 h-3 rounded-full"
                        style={{ backgroundColor: VIEWER_COLORS[i % VIEWER_COLORS.length] }}
                      />
                      <span className="text-sm text-text-secondary">{type.label}</span>
                    </div>
                    <span className="text-sm font-medium text-white">{type.percentage}%</span>
                  </div>
                ))}
              </div>
            </div>
          </div>
        </div>

        {/* Right Column - Stats */}
        <div className="space-y-4">
          {/* Language */}
          <div className="bg-background rounded-lg p-4">
            <div className="flex items-center justify-between mb-2">
              <span className="text-sm text-text-secondary flex items-center gap-2">
                <Globe className="w-4 h-4" />
                Primäre Sprache
              </span>
              <span className="text-xs text-text-secondary">{data.languageConfidence}% sicher</span>
            </div>
            <div className="text-xl font-bold text-white">{data.primaryLanguage}</div>
            <div className="mt-2 h-1.5 bg-border rounded-full overflow-hidden">
              <motion.div
                initial={{ width: 0 }}
                animate={{ width: `${data.languageConfidence}%` }}
                transition={{ delay: 0.3, duration: 0.5 }}
                className="h-full bg-primary rounded-full"
              />
            </div>
          </div>

          {/* Activity Pattern */}
          <div className="bg-background rounded-lg p-4">
            <div className="flex items-center gap-2 text-sm text-text-secondary mb-2">
              <Activity className="w-4 h-4" />
              Aktivitätsmuster
            </div>
            <div className="text-xl font-bold text-white">{activityLabels[data.activityPattern]}</div>
            <div className="mt-2 text-sm text-text-secondary">
              {hasPeakData
                ? `Peak-Zeiten (${data.timezone || 'UTC'}): ${normalizedPeakHours.map(h => `${h}:00`).join(', ')} Uhr`
                : 'Peak-Zeiten: zu wenig Aktivitätsdaten'}
            </div>
            {data.peakHoursMethod && (
              <div className="mt-1 text-xs text-text-secondary">Methode: {data.peakHoursMethod}</div>
            )}
          </div>

          {/* Engagement Scores */}
          <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
            <div className="bg-background rounded-lg p-4 text-center">
              <div className="flex items-center justify-center gap-1 text-text-secondary text-sm mb-2">
                <TrendingUp className="w-4 h-4" />
                Chat Penetration
              </div>
              <div className="text-2xl font-bold text-primary">
                {interactionReliable && interactionRate !== null ? `${interactionRate.toFixed(1)}%` : 'Nicht belastbar'}
              </div>
              <div className="text-xs text-text-secondary mt-1">
                {interactionReliable
                  ? 'Aktive Chatters / getrackte Chat-Accounts'
                  : `Nicht belastbar (Coverage ${interactionCoveragePct.toFixed(1)}%)`}
              </div>
            </div>
            <div className="bg-background rounded-lg p-4 text-center">
              <div className="flex items-center justify-center gap-1 text-text-secondary text-sm mb-2">
                <TrendingUp className="w-4 h-4" />
                Msg / 100 Viewer-Minuten
              </div>
              <div className="text-2xl font-bold text-accent">
                {messagesPer100ViewerMinutes !== null ? messagesPer100ViewerMinutes.toFixed(1) : '-'}
              </div>
              <div className="text-xs text-text-secondary mt-1">
                Coverage {(chattersCoverage * 100).toFixed(1)}%
              </div>
            </div>
            <div className="bg-background rounded-lg p-4 text-center">
              <div className="flex items-center justify-center gap-1 text-text-secondary text-sm mb-2">
                <Heart className="w-4 h-4" />
                Loyalitätsscore
              </div>
              <div className="text-2xl font-bold text-success">{data.loyaltyScore.toFixed(1)}%</div>
              <div className="text-xs text-text-secondary mt-1">Wiederkehrende</div>
            </div>
          </div>

          {/* Peak Hours Visualization */}
          <div className="bg-background rounded-lg p-4">
            <div className="text-sm text-text-secondary mb-3 flex items-center gap-2">
              <Clock className="w-4 h-4" />
              Beste Sendezeiten {data.timezone ? `(${data.timezone})` : '(UTC)'}
            </div>
            {hasPeakData ? (
              <>
                <div className="flex gap-1">
                  {Array.from({ length: 24 }, (_, i) => {
                    const isPeak = normalizedPeakHours.includes(i);
                    const isNearPeak = normalizedPeakHours.some(h => Math.abs(h - i) === 1);
                    return (
                      <motion.div
                        key={i}
                        initial={{ height: 0 }}
                        animate={{ height: isPeak ? 32 : isNearPeak ? 20 : 8 }}
                        transition={{ delay: i * 0.02 }}
                        className={`flex-1 rounded-sm ${
                          isPeak ? 'bg-primary' : isNearPeak ? 'bg-primary/50' : 'bg-border'
                        }`}
                        title={`${i}:00 Uhr${isPeak ? ' (Peak)' : ''}`}
                      />
                    );
                  })}
                </div>
                <div className="flex justify-between mt-1 text-xs text-text-secondary">
                  <span>0:00</span>
                  <span>6:00</span>
                  <span>12:00</span>
                  <span>18:00</span>
                  <span>24:00</span>
                </div>
              </>
            ) : (
              <div className="text-sm text-text-secondary border border-border rounded-lg p-3">
                Keine belastbaren Peak-Zeiten: zu wenig Aktivitäts-Samples.
              </div>
            )}
          </div>
        </div>
      </div>

      {/* Insights */}
      <div className="mt-6 pt-4 border-t border-border">
        <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
          {data.loyaltyScore > 40 && (
            <InsightBadge
              type="success"
              text={`Starke Community! ${data.loyaltyScore.toFixed(0)}% deiner Viewer kommen regelmäßig zurück.`}
            />
          )}
          {chatMethod === 'real_samples' && interactionReliable && (interactionRate ?? 0) > 15 && (
            <InsightBadge
              type="success"
              text={`Hohe Chat-Penetration: ${(interactionRate ?? 0).toFixed(0)}% deiner Chat-Audience interagiert aktiv.`}
            />
          )}
          {chatMethod !== 'real_samples' && (
            <InsightBadge
              type="info"
              text="Chat-Penetration und Conversion werden mit höherer Chatters-Coverage belastbarer."
            />
          )}
        </div>
      </div>
    </motion.div>
  );
}

interface InsightBadgeProps {
  type: 'success' | 'warning' | 'info';
  text: string;
}

function InsightBadge({ type, text }: InsightBadgeProps) {
  const styles = {
    success: 'bg-success/10 border-success/20 text-success',
    warning: 'bg-warning/10 border-warning/20 text-warning',
    info: 'bg-primary/10 border-primary/20 text-primary',
  };

  return (
    <div className={`px-3 py-2 rounded-lg border text-xs ${styles[type]}`}>
      {text}
    </div>
  );
}

export default AudienceDemographics;
