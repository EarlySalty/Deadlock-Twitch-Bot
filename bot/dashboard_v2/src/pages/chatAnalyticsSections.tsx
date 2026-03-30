import type { ComponentProps } from 'react';
import { useId } from 'react';
import { motion } from 'framer-motion';
import {
  MessageCircle,
  Users,
  Heart,
  TrendingUp,
  AlertCircle,
  Award,
  Info,
  Zap,
  Smile,
  AtSign,
} from 'lucide-react';
import {
  ComposedChart,
  Bar,
  Line,
  XAxis,
  YAxis,
  Tooltip,
  ResponsiveContainer,
  AreaChart,
  Area,
  PieChart,
  Pie,
  Cell,
} from 'recharts';

import { ViewerProfiles } from '@/components/charts/ViewerProfiles';
import { PlanGateCard } from '@/components/cards/PlanGateCard';
import type {
  ChatAnalytics as ChatAnalyticsType,
  CoachingData,
  ChatHypeTimeline,
  ChatContentAnalysis,
  ChatSocialGraph,
  RawChatStatus,
  TimeRange,
} from '@/types/analytics';

import type { ChatAnalyticsViewModel } from './chatAnalyticsViewModel';

const CHAT_PENETRATION_ENABLED = false;

function RawChatStatusBanner({
  status,
  compact = false,
}: {
  status?: RawChatStatus;
  compact?: boolean;
}) {
  if (!status) {
    return null;
  }
  if (!status.suspectedIngestionIssue && status.available !== false && !status.note) {
    return null;
  }

  return (
    <div
      className={`rounded-2xl border ${
        status.suspectedIngestionIssue
          ? 'border-warning/30 bg-warning/10 text-warning'
          : 'border-white/10 bg-white/[0.04] text-text-secondary'
      } ${compact ? 'mb-4 px-4 py-3 text-sm' : 'px-5 py-4 text-sm'}`}
    >
      <div className="flex items-start gap-3">
        <AlertCircle className={`${compact ? 'mt-0.5 h-4 w-4' : 'mt-0.5 h-5 w-5'} shrink-0`} />
        <div>
          <p className="font-medium text-white">
            {status.suspectedIngestionIssue
              ? 'Roh-Chat-Lücke erkannt'
              : 'Keine Roh-Chat-Nachrichten im Zeitraum'}
          </p>
          <p className="mt-1 leading-5">
            {status.note || 'Message-basierte KPIs und Charts sind für diesen Zeitraum eingeschränkt.'}
          </p>
        </div>
      </div>
    </div>
  );
}

export interface ChatAnalyticsContentProps {
  data: ChatAnalyticsType;
  days: TimeRange;
  model: ChatAnalyticsViewModel;
  viewerProfilesData: ComponentProps<typeof ViewerProfiles>['data'];
  coachingData?: CoachingData;
  selectedSessionId?: number;
  setSelectedSessionId: (id: number | undefined) => void;
  hypeData?: ChatHypeTimeline;
  contentData?: ChatContentAnalysis;
  socialData?: ChatSocialGraph;
  chatSocialGraphEnabled: boolean;
}

export function ChatAnalyticsContent({
  data,
  days,
  model,
  viewerProfilesData,
  coachingData,
  selectedSessionId,
  setSelectedSessionId,
  hypeData,
  contentData,
  socialData,
  chatSocialGraphEnabled,
}: ChatAnalyticsContentProps) {
  const {
    totalChatters,
    totalTrackedViewers,
    firstTimeChatters,
    returningChatters,
    returningTrackedViewers,
    coreLoyalViewers,
    silentCoreLoyalViewers,
    coreLoyalViewerRate,
    loyaltySessionThreshold,
    messagesPer100ViewerMinutes,
    messagesGaugeHasBenchmark,
    chatterReturnRate,
    interactionRateReliable,
    interactionCoverage,
    hourlyChartData,
    hasHourlySamples,
    hoursWithData,
    peakHour,
    dataMethod,
    noReturnHistory,
    chattersApiInactive,
    activeChattersShare,
    activeChattersDescription,
    chatPenetrationGaugeValue,
    messagesBenchmarkText,
    messagesBenchmarkFootnote,
    messagesGaugeProgress,
    newViewerShare,
    hourlyChartGradientId,
    chatAudienceTooltip,
  } = model;

  return (
    <div className="space-y-6">
      <RawChatStatusBanner status={data.rawChatStatus} />

      {dataMethod !== 'real_samples' && (
        <div className="panel-card rounded-2xl p-4 text-sm text-text-secondary">
          Datenqualität eingeschränkt: mindestens eine KPI basiert auf Low-Coverage/Fallback-Samples.
        </div>
      )}
      {CHAT_PENETRATION_ENABLED && (
        chattersApiInactive ? (
          <div className="panel-card rounded-2xl p-4 text-sm flex items-start gap-3 text-text-secondary">
            <Info className="w-4 h-4 text-primary shrink-0 mt-0.5" />
            <div>
              <span className="text-white font-medium">Chatters-API nicht aktiv</span>
              <span className="ml-2">— Chat Penetration kann nicht berechnet werden. Daten stammen nur aus Chat-Nachrichten (kein passive Viewer Tracking).</span>
            </div>
          </div>
        ) : !interactionRateReliable && totalTrackedViewers > 0 && (
          <div className="panel-card rounded-2xl p-4 text-sm text-text-secondary">
            Chat Penetration ist derzeit nicht belastbar: passive Samples oder Chatters-Coverage sind zu gering ({(interactionCoverage * 100).toFixed(1)}% Coverage).
          </div>
        )
      )}

      {/* Chatter Loyalty Distribution */}
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        className="panel-card rounded-2xl p-6 lg:px-7 lg:py-8 lg:min-h-[21rem] flex flex-col"
      >
        <div className="flex items-center gap-3 mb-6">
          <Heart className="w-6 h-6 text-primary" />
          <h2 className="text-xl font-bold text-white">Community-Treue</h2>
        </div>

        <div className="space-y-7 flex-1 flex flex-col justify-center">
          {noReturnHistory && (
            <div className="flex items-center gap-3 p-4 rounded-xl bg-primary/10 border border-primary/20">
              <Info className="w-5 h-5 text-primary shrink-0" />
              <div>
                <p className="text-white font-medium text-sm">Noch zu wenig Historie</p>
                <p className="text-text-secondary text-sm mt-0.5">
                  Alle {totalChatters} Chatter wurden erstmalig gesehen. Sobald sie wiederkehren, werden Return-Rate und Stammzuschauer berechnet.
                </p>
              </div>
            </div>
          )}

          <div
            className={`grid grid-cols-1 sm:grid-cols-2 ${
              CHAT_PENETRATION_ENABLED ? 'xl:grid-cols-6' : 'lg:grid-cols-5'
            } gap-4 lg:gap-3 items-start justify-items-center`}
          >
            <LoyaltyGauge
              label="Neue Zuschauer"
              percentage={newViewerShare}
              description="Chatten zum ersten Mal"
              startColor="var(--color-accent)"
              endColor="var(--color-primary)"
            />
            <LoyaltyGauge
              label="Stammzuschauer"
              percentage={coreLoyalViewerRate}
              valueText={`${coreLoyalViewerRate.toFixed(1)}%`}
              description={
                coreLoyalViewers > 0
                  ? `${coreLoyalViewers.toLocaleString('de-DE')} von ${totalTrackedViewers.toLocaleString('de-DE')} getrackten Zuschauern · ${silentCoreLoyalViewers.toLocaleString('de-DE')} silent · ${loyaltySessionThreshold}+ Streams`
                  : `Noch keine Stammzuschauer · ${loyaltySessionThreshold}+ Streams`
              }
              startColor="var(--color-success)"
              endColor="var(--color-accent)"
            />
            {CHAT_PENETRATION_ENABLED && (
              <LoyaltyGauge
                label="Chat Penetration"
                percentage={chatPenetrationGaugeValue ?? 0}
                description={
                  chattersApiInactive
                    ? 'Fallback-Metrik (eingeschränkt)'
                    : interactionRateReliable
                      ? 'Aktive Chatters / getrackte Chat-Accounts'
                      : `Nicht belastbar (${(interactionCoverage * 100).toFixed(1)}% Coverage)`
                }
                startColor="var(--color-primary)"
                endColor="var(--color-success)"
              />
            )}
            <LoyaltyGauge
              label="Messages pro 100 Viewer-Minuten"
              percentage={messagesGaugeProgress}
              valueText={
                messagesPer100ViewerMinutes !== null
                  ? messagesGaugeHasBenchmark
                    ? `${messagesGaugeProgress.toFixed(0)}%`
                    : messagesPer100ViewerMinutes.toFixed(1)
                  : '-'
              }
              description={messagesBenchmarkText}
              footnote={messagesBenchmarkFootnote}
              startColor="var(--color-success)"
              endColor="var(--color-primary)"
            />
            <LoyaltyGauge
              label="Wiederkehrende Chatters"
              percentage={chatterReturnRate}
              valueText={`${chatterReturnRate.toFixed(1)}%`}
              description={`${returningChatters.toLocaleString('de-DE')} von ${totalChatters.toLocaleString('de-DE')} aktiven Chattern · Erstmalig: ${firstTimeChatters.toLocaleString('de-DE')}`}
              startColor="var(--color-warning)"
              endColor="var(--color-accent)"
            />
            {CHAT_PENETRATION_ENABLED && (
              <LoyaltyGauge
                label="Chat Penetration"
                percentage={chatPenetrationGaugeValue ?? 0}
                valueText={
                  chatPenetrationGaugeValue !== null
                    ? `${chatPenetrationGaugeValue.toFixed(1)}%`
                    : 'N/A'
                }
                description={
                  chattersApiInactive
                    ? data.legacyInteractionActivePerAvgViewer != null
                      ? 'Fallback: aktive Chatter / Ø Viewer (eingeschränkt)'
                      : 'Chatters-API nicht aktiv'
                    : `Coverage ${(interactionCoverage * 100).toFixed(1)}% · ${chatAudienceTooltip}`
                }
                startColor="var(--color-primary)"
                endColor="var(--color-success)"
              />
            )}
            <LoyaltyGauge
              label="Aktive Chatters"
              percentage={activeChattersShare}
              valueText={`${activeChattersShare.toFixed(1)}%`}
              description={activeChattersDescription}
              startColor="var(--color-accent)"
              endColor="var(--color-primary)"
            />
          </div>
        </div>
      </motion.div>

      {/* Message Analysis */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        {/* Message Types */}
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.1 }}
          className="panel-card rounded-2xl p-6"
        >
          <div className="flex items-center gap-3 mb-6">
            <MessageCircle className="w-6 h-6 text-accent" />
            <h2 className="text-xl font-bold text-white">Nachrichtentypen</h2>
          </div>
          <div className="space-y-4">
            {data.messageTypes?.map((type) => (
              <div key={type.type}>
                <div className="flex justify-between text-sm mb-1">
                  <span className="text-text-secondary">{type.type}</span>
                  <span className="text-white font-medium">{type.percentage}% ({type.count})</span>
                </div>
                <div className="h-2 bg-background/80 rounded-full overflow-hidden">
                  <motion.div
                    initial={{ width: 0 }}
                    animate={{ width: `${type.percentage}%` }}
                    transition={{ duration: 1, ease: "easeOut" }}
                    className="h-full bg-gradient-to-r from-primary to-accent"
                  />
                </div>
              </div>
            ))}
            {(!data.messageTypes || data.messageTypes.length === 0) && (
               <p className="text-text-secondary text-center py-4">Keine Daten verfügbar</p>
            )}
          </div>
        </motion.div>

        {/* Hourly Activity */}
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.15 }}
          className="panel-card rounded-2xl p-6"
        >
          <div className="mb-6 flex items-start justify-between gap-4">
            <div>
              <div className="flex items-center gap-3">
                <TrendingUp className="w-6 h-6 text-success" />
                <h2 className="text-xl font-bold text-white">Chat-Nachrichten nach Uhrzeit</h2>
                <span className="text-[11px] text-text-secondary border border-border/60 rounded-full px-2 py-0.5">
                  {data.timezone || 'UTC'}
                </span>
              </div>
              <p className="mt-2 text-sm text-text-secondary">
                Aggregierte Roh-Chat-Nachrichten pro Stunde im gewählten Zeitraum.
              </p>
            </div>
          </div>
          {hasHourlySamples ? (
            <>
              {hoursWithData < 3 && (
                <div className="mb-3 text-xs text-text-secondary bg-background/50 rounded-lg px-3 py-2 border border-border/50">
                  Nur {hoursWithData} Stunde{hoursWithData !== 1 ? 'n' : ''} mit Daten — zu wenig für aussagekräftige Tageszeit-Analyse.
                </div>
              )}
              <div className="h-72 rounded-xl border border-border/60 bg-background/35 p-3">
                <ResponsiveContainer width="100%" height="100%">
                  <AreaChart
                    data={hourlyChartData}
                    margin={{ top: 12, right: 8, left: -16, bottom: 0 }}
                  >
                    <defs>
                      <linearGradient id={hourlyChartGradientId} x1="0" y1="0" x2="0" y2="1">
                        <stop offset="5%" stopColor="var(--color-success)" stopOpacity={0.4} />
                        <stop offset="95%" stopColor="var(--color-success)" stopOpacity={0.04} />
                      </linearGradient>
                    </defs>
                    <XAxis
                      dataKey="label"
                      interval={3}
                      tick={{ fontSize: 11, fill: 'var(--color-text-secondary)' }}
                      tickLine={false}
                      axisLine={false}
                    />
                    <YAxis
                      allowDecimals={false}
                      width={42}
                      tick={{ fontSize: 11, fill: 'var(--color-text-secondary)' }}
                      tickLine={false}
                      axisLine={false}
                    />
                    <Tooltip
                      contentStyle={CHART_TOOLTIP_STYLE}
                      labelFormatter={(label) => `${label} Uhr`}
                      formatter={(value) => {
                        const numericValue = Number(value);
                        return [
                          `${numericValue.toLocaleString('de-DE')} Nachrichten`,
                          'Chatvolumen',
                        ];
                      }}
                    />
                    <Area
                      type="monotone"
                      dataKey="count"
                      stroke="var(--color-success)"
                      strokeWidth={3}
                      fill={`url(#${hourlyChartGradientId})`}
                      activeDot={{ r: 5, strokeWidth: 0, fill: 'var(--color-success)' }}
                    />
                  </AreaChart>
                </ResponsiveContainer>
              </div>
              <div className="mt-3 flex flex-wrap items-center justify-between gap-2 text-xs text-text-secondary">
                <span>Jeder Punkt summiert alle Roh-Chat-Nachrichten dieser Stunde.</span>
                <span>
                  Peak bei {peakHour.hour}:00 Uhr mit {peakHour.count.toLocaleString('de-DE')} Nachrichten
                </span>
              </div>
            </>
          ) : (
            <div className="h-64 rounded-lg border border-border bg-background/50 p-4 text-sm text-text-secondary flex items-center justify-center text-center">
              Keine belastbaren Stundenmuster: zu wenig valide Chat-Timestamps.
            </div>
          )}
        </motion.div>
      </div>

      {/* Top Chatters Leaderboard */}
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ delay: 0.2 }}
        className="panel-card rounded-2xl p-6"
      >
        <div className="flex items-center gap-3 mb-6">
          <Award className="w-6 h-6 text-warning" />
          <h2 className="text-xl font-bold text-white">Top Chatter</h2>
        </div>

        {data.topChatters && data.topChatters.length > 0 ? (
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
            {data.topChatters.slice(0, 12).map((chatter, i) => (
              <ChatterCard
                key={chatter.login}
                rank={i + 1}
                login={chatter.login}
                messages={chatter.totalMessages}
                sessions={chatter.totalSessions}
                loyaltyScore={chatter.loyaltyScore}
              />
            ))}
          </div>
        ) : (
          <div className="text-center py-8 text-text-secondary">
            <Users className="w-12 h-12 mx-auto mb-3 opacity-50" />
            <p>Keine Chatter-Daten vorhanden</p>
          </div>
        )}
      </motion.div>

      {/* Zuschauer-Profile */}
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ delay: 0.25 }}
        className="panel-card rounded-2xl p-6"
      >
        <div className="flex items-center gap-3 mb-6">
          <Users className="w-6 h-6 text-accent" />
          <h2 className="text-xl font-bold text-white">Zuschauer-Profile</h2>
        </div>
        <ViewerProfiles data={viewerProfilesData} />
      </motion.div>

      {/* Chat-Konzentration */}
      {coachingData && !coachingData.empty && (
        <ChatConcentrationSection data={coachingData} />
      )}

      {/* Insights */}
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ delay: 0.3 }}
        className="rounded-2xl border border-primary/25 bg-gradient-to-r from-primary/16 via-card to-accent/16 p-6"
      >
        <h3 className="font-bold text-white mb-4">Chat-Insights</h3>
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          {totalChatters > 0 ? (
            <>
              <InsightItem
                type={chatterReturnRate > 30 ? 'positive' : 'warning'}
                text={chatterReturnRate > 30
                  ? `Starke Community! ${chatterReturnRate.toFixed(0)}% deiner Chatter kommen wieder.`
                  : `${chatterReturnRate.toFixed(0)}% Return Rate - versuche mehr Interaktion!`}
              />
              <InsightItem
                type={coreLoyalViewers > 0 ? 'positive' : 'info'}
                text={coreLoyalViewers > 0
                  ? silentCoreLoyalViewers > 0
                    ? `${coreLoyalViewers.toLocaleString('de-DE')} Stammzuschauer erkannt, davon ${silentCoreLoyalViewers.toLocaleString('de-DE')} silent.`
                    : `${coreLoyalViewers.toLocaleString('de-DE')} Stammzuschauer im ${days}-Tage-Fenster erkannt.`
                  : `${returningTrackedViewers.toLocaleString('de-DE')} wiederkehrende Zuschauer, aber noch niemand mit ${loyaltySessionThreshold}+ Streams im Fenster.`
                }
              />
            </>
          ) : (
            <InsightItem
              type="info"
              text="Keine aktiven Chatter im Zeitraum: Erst bei echten Chat-Samples werden Treue-Insights angezeigt."
            />
          )}
        </div>
      </motion.div>

      {/* ═══ Chat Deep Analysis Sections ═══ */}

      {/* Hype-Momente */}
      <PlanGateCard featureId="hype_timeline" title="Hype-Timeline">
        {hypeData && <HypeMomenteSection data={hypeData} selectedSessionId={selectedSessionId} onSessionChange={setSelectedSessionId} />}
      </PlanGateCard>

      {/* Stimmung & Topics */}
      <PlanGateCard featureId="chat_content_analysis" title="Chat-Inhaltsanalyse">
        {contentData && <StimmungTopicsSection data={contentData} />}
      </PlanGateCard>

      {/* Chat-Netzwerk ist bewusst deaktiviert; Implementierung bleibt vorerst im Code. */}
      {chatSocialGraphEnabled && (
        <PlanGateCard featureId="chat_social_graph" title="Chat Social Graph">
          {socialData && <ChatNetzwerkSection data={socialData} />}
        </PlanGateCard>
      )}
    </div>
  );
}

interface LoyaltyGaugeProps {
  label: string;
  percentage: number;
  description: string;
  startColor: string;
  endColor: string;
  valueText?: string;
  footnote?: string;
}

function LoyaltyGauge({
  label,
  percentage,
  description,
  startColor,
  endColor,
  valueText,
  footnote,
}: LoyaltyGaugeProps) {
  const clampedPercentage = Math.min(100, Math.max(0, percentage));
  const gradientId = `gauge-${useId().replace(/:/g, '')}`;
  const centerValue = valueText ?? `${clampedPercentage.toFixed(0)}%`;
  const centerTextClass = centerValue.length > 6 ? 'text-lg' : centerValue.length > 4 ? 'text-xl' : 'text-2xl';

  return (
    <div className="w-full max-w-[12rem] text-center">
      <div className="relative w-32 h-32 mx-auto mb-3">
        <svg className="w-full h-full -rotate-90">
          <circle
            cx="64"
            cy="64"
            r="56"
            fill="none"
            stroke="currentColor"
            strokeWidth="12"
            className="text-border"
          />
          <motion.circle
            cx="64"
            cy="64"
            r="56"
            fill="none"
            stroke={`url(#${gradientId})`}
            strokeWidth="12"
            strokeLinecap="round"
            strokeDasharray={`${(clampedPercentage / 100) * 352} 352`}
            initial={{ strokeDasharray: '0 352' }}
            animate={{ strokeDasharray: `${(clampedPercentage / 100) * 352} 352` }}
            transition={{ duration: 1, delay: 0.3 }}
          />
          <defs>
            <linearGradient id={gradientId} x1="0%" y1="0%" x2="100%" y2="0%">
              <stop offset="0%" stopColor={startColor} />
              <stop offset="100%" stopColor={endColor} />
            </linearGradient>
          </defs>
        </svg>
        <div className="absolute inset-0 flex items-center justify-center">
          <span className={`${centerTextClass} font-bold text-white`}>{centerValue}</span>
        </div>
      </div>
      <div className="font-medium text-white">{label}</div>
      <div className="text-sm text-text-secondary">{description}</div>
      {footnote && <div className="mt-1 text-xs text-text-secondary/80">{footnote}</div>}
    </div>
  );
}

interface ChatterCardProps {
  rank: number;
  login: string;
  messages: number;
  sessions: number;
  loyaltyScore: number;
}

function ChatterCard({ rank, login, messages, sessions, loyaltyScore }: ChatterCardProps) {
  const getRankStyle = () => {
    if (rank === 1) return 'bg-gradient-to-br from-yellow-400 to-yellow-600 text-black';
    if (rank === 2) return 'bg-gradient-to-br from-gray-300 to-gray-500 text-black';
    if (rank === 3) return 'bg-gradient-to-br from-amber-600 to-amber-800 text-white';
    return 'bg-border text-text-secondary';
  };

  return (
    <motion.div
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ delay: rank * 0.03 }}
      className="flex items-center gap-3 p-3 rounded-xl bg-background/75 border border-border/65"
    >
      <div className={`w-8 h-8 rounded-full flex items-center justify-center text-sm font-bold ${getRankStyle()}`}>
        {rank}
      </div>
      <div className="flex-1 min-w-0">
        <div className="font-medium text-white truncate">{login}</div>
        <div className="text-xs text-text-secondary">
          {messages.toLocaleString('de-DE')} Nachrichten • {sessions} Sessions
        </div>
      </div>
      <div className="text-right">
        <div className="text-sm font-medium text-primary">{loyaltyScore}</div>
        <div className="text-xs text-text-secondary">Loyalität</div>
      </div>
    </motion.div>
  );
}

interface InsightItemProps {
  type: 'positive' | 'warning' | 'info';
  text: string;
}

function InsightItem({ type, text }: InsightItemProps) {
  const styles = {
    positive: 'bg-success/10 border-success/20 text-success',
    warning: 'bg-warning/10 border-warning/20 text-warning',
    info: 'bg-primary/10 border-primary/20 text-primary',
  };

  return (
    <div className={`p-3 rounded-lg border ${styles[type]}`}>
      <p className="text-sm text-white">{text}</p>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Chat-Konzentration (moved from Coaching tab)
// ---------------------------------------------------------------------------

const LOYALTY_LABELS: Record<string, { label: string; color: string }> = {
  oneTimer: { label: 'Einmalig', color: 'bg-error/60' },
  casual: { label: 'Gelegentlich', color: 'bg-warning/60' },
  regular: { label: 'Regulär', color: 'bg-primary/60' },
  loyal: { label: 'Loyal', color: 'bg-success/60' },
};

function ChatConcentrationSection({ data }: { data: CoachingData }) {
  const chat = data.chatConcentration;
  if (!chat || chat.totalChatters === 0) return null;

  const bucketOrder = ['oneTimer', 'casual', 'regular', 'loyal'];
  const buckets = bucketOrder
    .filter(k => chat.loyaltyBuckets[k])
    .map(k => ({ key: k, ...chat.loyaltyBuckets[k], ...LOYALTY_LABELS[k] }));

  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ delay: 0.28 }}
      className="panel-card rounded-2xl p-6"
    >
      <div className="flex items-center gap-3 mb-6">
        <MessageCircle className="w-6 h-6 text-primary" />
        <h2 className="text-xl font-bold text-white">Chat-Konzentration</h2>
      </div>

      {/* KPIs */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-4 mb-6">
        <div className="text-center p-3 rounded-lg bg-background/50">
          <div className={`text-2xl font-bold ${chat.top1Pct > 50 ? 'text-error' : chat.top1Pct > 30 ? 'text-warning' : 'text-success'}`}>
            {chat.top1Pct}%
          </div>
          <div className="text-xs text-text-secondary">Top-1 Chatter Anteil</div>
        </div>
        <div className="text-center p-3 rounded-lg bg-background/50">
          <div className={`text-2xl font-bold ${chat.top3Pct > 70 ? 'text-warning' : 'text-white'}`}>
            {chat.top3Pct}%
          </div>
          <div className="text-xs text-text-secondary">Top-3 kumulativ</div>
        </div>
        <div className="text-center p-3 rounded-lg bg-background/50">
          <div className="text-2xl font-bold text-white">{chat.msgsPerChatter}</div>
          <div className="text-xs text-text-secondary">Msgs / Chatter</div>
        </div>
        <div className="text-center p-3 rounded-lg bg-background/50">
          <div className={`text-2xl font-bold ${chat.concentrationIndex > 2500 ? 'text-error' : 'text-white'}`}>
            {chat.concentrationIndex.toLocaleString()}
          </div>
          <div className="text-xs text-text-secondary">HHI-Index</div>
        </div>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        {/* Loyalty Buckets */}
        <div>
          <h3 className="text-sm font-medium text-text-secondary mb-3">Chatter-Loyalität</h3>
          <div className="flex rounded-lg overflow-hidden h-8 mb-3">
            {buckets.map(b => (
              <div
                key={b.key}
                className={`${b.color} flex items-center justify-center text-xs text-white font-medium`}
                style={{ width: `${b.pct}%`, minWidth: b.pct > 5 ? undefined : '2px' }}
                title={`${b.label}: ${b.count} (${b.pct}%)`}
              >
                {b.pct >= 10 && `${b.pct}%`}
              </div>
            ))}
          </div>
          <div className="flex flex-wrap gap-3 text-xs">
            {buckets.map(b => (
              <span key={b.key} className="flex items-center gap-1.5 text-text-secondary">
                <span className={`w-2.5 h-2.5 rounded-sm ${b.color}`} />
                {b.label}: {b.count} ({b.pct}%)
              </span>
            ))}
          </div>
          <div className="mt-3 text-xs text-text-secondary">
            Einmaliger Chatter-Anteil: <span className={chat.ownOneTimerPct > chat.avgPeerOneTimerPct ? 'text-warning' : 'text-success'}>
              {chat.ownOneTimerPct}%
            </span>
            {' '}vs Kategorie-Schnitt {chat.avgPeerOneTimerPct}%
          </div>
        </div>

        {/* Top Chatters by share */}
        <div>
          <h3 className="text-sm font-medium text-text-secondary mb-3">Nachrichten-Anteil Top-Chatter</h3>
          <div className="space-y-2">
            {chat.topChatters.slice(0, 5).map((c, i) => (
              <div key={c.login} className="flex items-center gap-3">
                <span className="text-xs text-text-secondary w-5 text-right">{i + 1}.</span>
                <span className="text-sm text-white flex-1 truncate">{c.login}</span>
                <div className="w-24 bg-background rounded-full h-2 overflow-hidden">
                  <div
                    className="h-full bg-primary/70 rounded-full"
                    style={{ width: `${Math.min(100, c.sharePct)}%` }}
                  />
                </div>
                <span className="text-xs text-text-secondary w-12 text-right">{c.sharePct.toFixed(1)}%</span>
              </div>
            ))}
          </div>
        </div>
      </div>
    </motion.div>
  );
}

// ---------------------------------------------------------------------------
// Hype-Momente Section
// ---------------------------------------------------------------------------

const CHART_TOOLTIP_STYLE = {
  backgroundColor: '#1f2937',
  border: '1px solid rgba(194,221,240,0.25)',
  borderRadius: '8px',
  fontSize: '12px',
};

function HypeMomenteSection({
  data,
  selectedSessionId,
  onSessionChange,
}: {
  data: ChatHypeTimeline;
  selectedSessionId?: number;
  onSessionChange: (id: number | undefined) => void;
}) {
  const correlationLabel = (() => {
    const r = data.correlation.chatViewerR;
    if (Math.abs(r) >= 0.7) return `stark (r=${r})`;
    if (Math.abs(r) >= 0.4) return `moderat (r=${r})`;
    if (Math.abs(r) >= 0.2) return `schwach (r=${r})`;
    return `keine (r=${r})`;
  })();

  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ delay: 0.35 }}
      className="panel-card rounded-2xl p-6"
    >
      <RawChatStatusBanner status={data.rawChatStatus} compact />

      <div className="flex items-center justify-between mb-6">
        <div className="flex items-center gap-3">
          <Zap className="w-6 h-6 text-warning" />
          <h2 className="text-xl font-bold text-white">Hype-Momente</h2>
        </div>
        {/* Session Selector */}
        {data.recentSessions.length > 0 && (
          <select
            className="bg-background border border-border rounded-lg px-3 py-1.5 text-sm text-white focus:outline-none focus:border-primary/50"
            value={selectedSessionId ?? data.sessionId}
            onChange={e => {
              const val = parseInt(e.target.value, 10);
              onSessionChange(val === data.sessionId && !selectedSessionId ? undefined : val);
            }}
          >
            <option value={data.sessionId}>
              Aktuelle Session — {data.sessionTitle || data.startedAt.split('T')[0]}
            </option>
            {data.recentSessions.map(s => (
              <option key={s.id} value={s.id}>
                {s.date} — {s.title || `Session #${s.id}`} (Ø {s.avgMPM} MPM)
              </option>
            ))}
          </select>
        )}
      </div>

      {/* KPI Row */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-4 mb-6">
        <div className="text-center p-3 rounded-lg bg-background/50">
          <div className="text-2xl font-bold text-white">{data.avgMPM}</div>
          <div className="text-xs text-text-secondary">Ø Messages/Min</div>
        </div>
        <div className="text-center p-3 rounded-lg bg-background/50">
          <div className="text-2xl font-bold text-warning">{data.peakMPM}</div>
          <div className="text-xs text-text-secondary">Peak MPM</div>
        </div>
        <div className="text-center p-3 rounded-lg bg-background/50">
          <div className="text-2xl font-bold text-accent">{data.spikes.length}</div>
          <div className="text-xs text-text-secondary">Hype-Spikes</div>
        </div>
        <div className="text-center p-3 rounded-lg bg-background/50">
          <div className={`text-sm font-bold ${Math.abs(data.correlation.chatViewerR) >= 0.4 ? 'text-success' : 'text-text-secondary'}`}>
            {correlationLabel}
          </div>
          <div className="text-xs text-text-secondary">Chat↔Viewer Korrelation</div>
        </div>
      </div>

      {/* Dual-Axis Chart */}
      {data.timeline.length > 0 && (
        <div className="h-[280px] mb-4">
          <ResponsiveContainer width="100%" height="100%">
            <ComposedChart data={data.timeline}>
              <XAxis
                dataKey="minute"
                tickFormatter={v => `${v}m`}
                tick={{ fontSize: 11, fill: 'var(--color-text-secondary)' }}
              />
              <YAxis yAxisId="left" tick={{ fontSize: 11, fill: 'var(--color-text-secondary)' }} />
              <YAxis yAxisId="right" orientation="right" tick={{ fontSize: 11, fill: 'var(--color-text-secondary)' }} />
              <Tooltip contentStyle={CHART_TOOLTIP_STYLE} labelFormatter={v => `Minute ${v}`} />
              <Bar
                yAxisId="left"
                dataKey="messages"
                fill="var(--color-primary)"
                opacity={0.7}
                radius={[2, 2, 0, 0]}
                name="Messages"
              />
              <Line
                yAxisId="right"
                type="monotone"
                dataKey="viewers"
                stroke="var(--color-accent)"
                strokeWidth={2}
                dot={false}
                name="Viewer"
              />
            </ComposedChart>
          </ResponsiveContainer>
        </div>
      )}

      {/* Top Spikes Table */}
      {data.spikes.length > 0 && (
        <div>
          <h3 className="text-sm font-medium text-text-secondary mb-2">Top Hype-Spikes</h3>
          <div className="space-y-1.5">
            {data.spikes.slice(0, 5).map((spike, i) => (
              <div key={i} className="flex items-center justify-between text-xs bg-background/50 rounded-lg px-3 py-2">
                <span className="text-white font-medium">Minute {spike.minute}</span>
                <span className="text-warning font-bold">{spike.messages} Messages</span>
                <span className="text-text-secondary">{spike.multiplier}x Durchschnitt</span>
              </div>
            ))}
          </div>
        </div>
      )}
    </motion.div>
  );
}

// ---------------------------------------------------------------------------
// Stimmung & Topics Section
// ---------------------------------------------------------------------------

const TOPIC_COLORS: Record<string, string> = {
  heroes: '#3b82f6',
  builds: '#f59e0b',
  ranked: '#8b5cf6',
  meta: '#ef4444',
  gameplay: '#06b6d4',
  backseat: '#f97316',
  commands: '#60a5fa',
  social: '#34d399',
  smalltalk: '#facc15',
  greeting: '#22d3ee',
  community: '#10b981',
  reaction: '#ec4899',
  other: '#6b7280',
};

const TOPIC_LABELS: Record<string, string> = {
  heroes: 'Heroes',
  builds: 'Builds',
  ranked: 'Ranked',
  meta: 'Meta',
  gameplay: 'Gameplay',
  backseat: 'Backseat',
  commands: 'Commands',
  social: 'Social',
  smalltalk: 'Smalltalk',
  greeting: 'Begruessung',
  community: 'Community',
  reaction: 'Reaktionen',
  other: 'Sonstiges',
};

function StimmungTopicsSection({ data }: { data: ChatContentAnalysis }) {
  const sentimentColor = data.overallSentiment.score > 0.2 ? 'text-success' : data.overallSentiment.score < -0.2 ? 'text-error' : 'text-text-secondary';
  const trendArrow = data.overallSentiment.trend === 'rising' ? '↑' : data.overallSentiment.trend === 'falling' ? '↓' : '→';

  // Donut data for topics
  const topicEntries = Object.entries(data.topicBreakdown).filter(([, v]) => v > 0);
  const topicTotal = topicEntries.reduce((s, [, v]) => s + v, 0);
  const donutData = topicEntries.map(([key, val]) => ({
    name: TOPIC_LABELS[key] || key,
    value: val,
    color: TOPIC_COLORS[key] || '#6b7280',
  }));

  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ delay: 0.4 }}
      className="panel-card rounded-2xl p-6"
    >
      <RawChatStatusBanner status={data.rawChatStatus} compact />

      <div className="flex items-center gap-3 mb-6">
        <Smile className="w-6 h-6 text-success" />
        <h2 className="text-xl font-bold text-white">Stimmung & Topics</h2>
        <span className={`text-sm font-bold ${sentimentColor}`}>
          {data.overallSentiment.label} {trendArrow}
        </span>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        {/* Sentiment Timeline */}
        <div>
          <h3 className="text-sm font-medium text-text-secondary mb-3">Stimmungsverlauf</h3>
          {data.sentimentTimeline.length > 0 ? (
            <div className="h-[200px]">
              <ResponsiveContainer width="100%" height="100%">
                <AreaChart data={data.sentimentTimeline}>
                  <XAxis dataKey="bucket" hide />
                  <YAxis domain={[-1, 1]} tick={{ fontSize: 10, fill: 'var(--color-text-secondary)' }} />
                  <Tooltip contentStyle={CHART_TOOLTIP_STYLE} labelFormatter={v => String(v)} />
                  <Area
                    type="monotone"
                    dataKey="score"
                    stroke="var(--color-success)"
                    fill="var(--color-success)"
                    fillOpacity={0.2}
                    strokeWidth={2}
                    name="Sentiment"
                  />
                </AreaChart>
              </ResponsiveContainer>
            </div>
          ) : (
            <p className="text-xs text-text-secondary py-8 text-center">Keine Sentiment-Daten</p>
          )}
          <div className="flex justify-between text-xs text-text-secondary mt-2">
            <span>Positiv: {data.overallSentiment.positiveCount}</span>
            <span>Negativ: {data.overallSentiment.negativeCount}</span>
            <span>Analysiert: {data.overallSentiment.totalAnalyzed}</span>
          </div>
        </div>

        {/* Hero Mentions + Topic Donut */}
        <div className="space-y-4">
          {/* Hero Mentions */}
          {data.heroMentions.length > 0 && (
            <div>
              <h3 className="text-sm font-medium text-text-secondary mb-2">Hero-Mentions</h3>
              <div className="space-y-2">
                {data.heroMentions.slice(0, 8).map(hero => (
                  <div key={hero.hero} className="flex items-center gap-2">
                    <span className="text-xs text-white w-24 truncate capitalize">{hero.hero.replace('_', ' ')}</span>
                    <div className="flex-1 h-2 bg-background/80 rounded-full overflow-hidden">
                      <div
                        className="h-full bg-primary/70 rounded-full"
                        style={{ width: `${hero.pct}%` }}
                      />
                    </div>
                    <span className="text-xs text-text-secondary w-16 text-right">{hero.count} ({hero.pct}%)</span>
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* Topic Donut */}
          {donutData.length > 0 && (
            <div>
              <h3 className="text-sm font-medium text-text-secondary mb-2">Topic-Verteilung</h3>
              <div className="flex items-center gap-4">
                <div className="h-[120px] w-[120px]">
                  <ResponsiveContainer width="100%" height="100%">
                    <PieChart>
                      <Pie data={donutData} innerRadius={30} outerRadius={50} dataKey="value" stroke="none">
                        {donutData.map((entry, idx) => (
                          <Cell key={idx} fill={entry.color} />
                        ))}
                      </Pie>
                      <Tooltip contentStyle={CHART_TOOLTIP_STYLE} formatter={(value) => `${Number(value).toLocaleString('de-DE')}`} />
                    </PieChart>
                  </ResponsiveContainer>
                </div>
                <div className="flex flex-wrap gap-2 text-xs">
                  {donutData.map((d, idx) => (
                    <span key={`${d.name}-${idx}`} className="flex items-center gap-1.5 text-text-secondary">
                      <span className="w-2.5 h-2.5 rounded-sm" style={{ backgroundColor: d.color }} />
                      {d.name}: {topicTotal > 0 ? Math.round(d.value / topicTotal * 100) : 0}%
                    </span>
                  ))}
                </div>
              </div>
            </div>
          )}
        </div>
      </div>

      {/* Backseat + Engagement Depth Row */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6 mt-6">
        {/* Backseat Gaming */}
        <div className="bg-background/30 rounded-xl border border-border/50 p-4">
          <div className="flex items-center justify-between mb-3">
            <h3 className="text-sm font-medium text-text-secondary">Backseat Gaming</h3>
            <span className={`text-xs font-bold px-2 py-0.5 rounded-full ${
              data.backseat.pct > 10 ? 'bg-warning/10 text-warning' : 'bg-success/10 text-success'
            }`}>
              {data.backseat.pct}%
            </span>
          </div>
          <div className="flex items-baseline gap-2 mb-3">
            <span className="text-2xl font-bold text-white">{data.backseat.count.toLocaleString('de-DE')}</span>
            <span className="text-xs text-text-secondary">Messages mit Coaching-Charakter</span>
          </div>
          {data.backseat.examples.length > 0 && (
            <div className="space-y-1 max-h-[100px] overflow-y-auto">
              {data.backseat.examples.slice(0, 5).map((ex, i) => (
                <div key={i} className="text-xs text-text-secondary bg-background/50 rounded px-2 py-1 truncate italic">
                  "{ex}"
                </div>
              ))}
            </div>
          )}
        </div>

        {/* Chat Engagement Depth */}
        <div className="bg-background/30 rounded-xl border border-border/50 p-4">
          <div className="flex items-center justify-between mb-3">
            <h3 className="text-sm font-medium text-text-secondary">Chat-Tiefe</h3>
            <span className="text-xs text-text-secondary">
              Ø {data.engagementDepth.avgWordCount} Wörter/Message
            </span>
          </div>
          {/* Stacked bar */}
          <div className="flex rounded-lg overflow-hidden h-8 mb-3">
            {data.engagementDepth.reactionPct > 0 && (
              <div
                className="bg-warning/60 flex items-center justify-center text-xs text-white font-medium"
                style={{ width: `${data.engagementDepth.reactionPct}%`, minWidth: data.engagementDepth.reactionPct > 8 ? undefined : '2px' }}
                title={`Reactions: ${data.engagementDepth.reaction}`}
              >
                {data.engagementDepth.reactionPct >= 10 && `${data.engagementDepth.reactionPct}%`}
              </div>
            )}
            {data.engagementDepth.shortPct > 0 && (
              <div
                className="bg-primary/60 flex items-center justify-center text-xs text-white font-medium"
                style={{ width: `${data.engagementDepth.shortPct}%`, minWidth: data.engagementDepth.shortPct > 8 ? undefined : '2px' }}
                title={`Kurz: ${data.engagementDepth.short}`}
              >
                {data.engagementDepth.shortPct >= 10 && `${data.engagementDepth.shortPct}%`}
              </div>
            )}
            {data.engagementDepth.discussionPct > 0 && (
              <div
                className="bg-success/60 flex items-center justify-center text-xs text-white font-medium"
                style={{ width: `${data.engagementDepth.discussionPct}%`, minWidth: data.engagementDepth.discussionPct > 8 ? undefined : '2px' }}
                title={`Diskussionen: ${data.engagementDepth.discussion}`}
              >
                {data.engagementDepth.discussionPct >= 10 && `${data.engagementDepth.discussionPct}%`}
              </div>
            )}
          </div>
          <div className="flex flex-wrap gap-3 text-xs">
            <span className="flex items-center gap-1.5 text-text-secondary">
              <span className="w-2.5 h-2.5 rounded-sm bg-warning/60" />
              Reactions (1-3): {data.engagementDepth.reaction.toLocaleString('de-DE')} ({data.engagementDepth.reactionPct}%)
            </span>
            <span className="flex items-center gap-1.5 text-text-secondary">
              <span className="w-2.5 h-2.5 rounded-sm bg-primary/60" />
              Kurz (4-10): {data.engagementDepth.short.toLocaleString('de-DE')} ({data.engagementDepth.shortPct}%)
            </span>
            <span className="flex items-center gap-1.5 text-text-secondary">
              <span className="w-2.5 h-2.5 rounded-sm bg-success/60" />
              Diskussion (11+): {data.engagementDepth.discussion.toLocaleString('de-DE')} ({data.engagementDepth.discussionPct}%)
            </span>
          </div>
        </div>
      </div>
    </motion.div>
  );
}

// ---------------------------------------------------------------------------
// Chat-Netzwerk Section
// ---------------------------------------------------------------------------

function ChatNetzwerkSection({ data }: { data: ChatSocialGraph }) {
  const shouldRenderEmptyState =
    data.totalMentions === 0 &&
    (data.rawChatStatus?.suspectedIngestionIssue ||
      data.rawChatStatus?.available === false ||
      Boolean(data.rawChatStatus?.note));

  if (data.totalMentions === 0 && !shouldRenderEmptyState) return null;

  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ delay: 0.45 }}
      className="panel-card rounded-2xl p-6"
    >
      <RawChatStatusBanner status={data.rawChatStatus} compact />

      <div className="flex items-center gap-3 mb-6">
        <AtSign className="w-6 h-6 text-accent" />
        <h2 className="text-xl font-bold text-white">Chat-Netzwerk</h2>
      </div>

      {data.totalMentions === 0 ? (
        <div className="rounded-xl border border-border/60 bg-background/40 px-4 py-8 text-center text-sm text-text-secondary">
          Keine belastbaren Mention-Daten im gewählten Zeitraum.
        </div>
      ) : (
        <>
          {/* Stats Row */}
          <div className="grid grid-cols-3 gap-4 mb-6">
            <div className="text-center p-3 rounded-lg bg-background/50">
              <div className="text-2xl font-bold text-white">{data.totalMentions}</div>
              <div className="text-xs text-text-secondary">@Mentions gesamt</div>
            </div>
            <div className="text-center p-3 rounded-lg bg-background/50">
              <div className="text-2xl font-bold text-accent">{data.uniqueMentioners}</div>
              <div className="text-xs text-text-secondary">Unique Mentioner</div>
            </div>
            <div className="text-center p-3 rounded-lg bg-background/50">
              <div className="text-2xl font-bold text-primary">{data.uniqueMentioned}</div>
              <div className="text-xs text-text-secondary">Erwähnte User</div>
            </div>
          </div>

          <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
            {/* Hub Cards */}
            <div>
              <h3 className="text-sm font-medium text-text-secondary mb-3">Conversation-Hubs</h3>
              <div className="space-y-2">
                {data.hubs.slice(0, 5).map((hub, i) => (
                  <div key={hub.login} className="flex items-center gap-3 p-3 rounded-xl bg-background/75 border border-border/65">
                    <div className="w-7 h-7 rounded-full bg-accent/20 flex items-center justify-center text-xs font-bold text-accent">
                      {i + 1}
                    </div>
                    <div className="flex-1 min-w-0">
                      <div className="font-medium text-white text-sm truncate">{hub.login}</div>
                      <div className="text-xs text-text-secondary">
                        {hub.mentionsSent} gesendet · {hub.mentionsReceived} erhalten
                      </div>
                    </div>
                    <div className="text-sm font-bold text-accent">{hub.score}</div>
                  </div>
                ))}
              </div>
            </div>

            {/* Top Pairs + Distribution */}
            <div className="space-y-4">
              <div>
                <h3 className="text-sm font-medium text-text-secondary mb-3">Top-Gespräche</h3>
                <div className="space-y-1.5">
                  {data.topPairs.slice(0, 8).map((pair, i) => (
                    <div key={i} className="flex items-center justify-between text-xs bg-background/50 rounded-lg px-3 py-2">
                      <span className="text-white">
                        <span className="font-medium">{pair.from}</span>
                        <span className="text-text-secondary mx-1.5">→</span>
                        <span className="font-medium">{pair.to}</span>
                      </span>
                      <span className="text-accent font-bold">{pair.count}x</span>
                    </div>
                  ))}
                </div>
              </div>

              <div>
                <h3 className="text-sm font-medium text-text-secondary mb-2">Mention-Verteilung</h3>
                <div className="flex gap-3 text-xs">
                  <span className="text-text-secondary">1x: <span className="text-white font-medium">{data.mentionDistribution.mentionedOnce}</span></span>
                  <span className="text-text-secondary">2-5x: <span className="text-white font-medium">{data.mentionDistribution.mentioned2to5}</span></span>
                  <span className="text-text-secondary">5+: <span className="text-white font-medium">{data.mentionDistribution.mentioned5plus}</span></span>
                </div>
              </div>
            </div>
          </div>
        </>
      )}
    </motion.div>
  );
}
