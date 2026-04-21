import type { ComponentProps } from 'react';
import { useId } from 'react';
import { motion } from 'framer-motion';
import { Award, Heart, Info, MessageCircle, TrendingUp, Users } from 'lucide-react';
import { Area, AreaChart, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts';

import { ViewerProfiles } from '@/components/charts/ViewerProfiles';
import { PlanGateCard } from '@/components/cards/PlanGateCard';
import type {
  ChatAnalytics as ChatAnalyticsType,
  ChatContentAnalysis,
  ChatHypeTimeline,
  ChatSocialGraph,
  CoachingData,
  TimeRange,
} from '@/types/analytics';

import type { ChatAnalyticsViewModel } from './chatAnalyticsViewModel';
import {
  ChatConcentrationSection,
  ChatNetzwerkSection,
  HypeMomenteSection,
  StimmungTopicsSection,
} from './chatAnalyticsDeepSections';
import { RawChatStatusBanner } from './chatAnalyticsShared';

const CHAT_PENETRATION_ENABLED = false;
const CHART_TOOLTIP_STYLE = {
  backgroundColor: 'rgba(9, 12, 22, 0.92)',
  border: '1px solid rgba(148, 163, 184, 0.18)',
  borderRadius: 16,
  boxShadow: '0 24px 60px rgba(2, 6, 23, 0.42)',
  color: '#f8fafc',
} as const;

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
    hourlyChartGradientId,
    chatAudienceTooltip,
    newViewerShare,
  } = model;

  return (
    <div className="space-y-6">
      <RawChatStatusBanner status={data.rawChatStatus} />

      {dataMethod !== 'real_samples' && (
        <div className="panel-card rounded-2xl p-4 text-sm text-text-secondary">
          Datenqualität eingeschränkt: mindestens eine KPI basiert auf Low-Coverage/Fallback-Samples.
        </div>
      )}

      {CHAT_PENETRATION_ENABLED &&
        (chattersApiInactive ? (
          <div className="panel-card flex items-start gap-3 rounded-2xl p-4 text-sm text-text-secondary">
            <Info className="mt-0.5 h-4 w-4 shrink-0 text-primary" />
            <div>
              <span className="font-medium text-white">Chatters-API nicht aktiv</span>
              <span className="ml-2">— Chat Penetration kann nicht berechnet werden. Daten stammen nur aus Chat-Nachrichten.</span>
            </div>
          </div>
        ) : !interactionRateReliable && totalTrackedViewers > 0 ? (
          <div className="panel-card rounded-2xl p-4 text-sm text-text-secondary">
            Chat Penetration ist derzeit nicht belastbar: passive Samples oder Chatters-Coverage sind zu gering ({(interactionCoverage * 100).toFixed(1)}% Coverage).
          </div>
        ) : null)}

      <motion.div initial={{ opacity: 0, y: 20 }} animate={{ opacity: 1, y: 0 }} className="panel-card flex min-h-[21rem] flex-col rounded-2xl p-6 lg:px-7 lg:py-8">
        <div className="mb-6 flex items-center gap-3">
          <Heart className="h-6 w-6 text-primary" />
          <h2 className="text-xl font-bold text-white">Community-Treue</h2>
        </div>

        <div className="flex flex-1 flex-col justify-center space-y-7">
          {noReturnHistory && (
            <div className="flex items-center gap-3 rounded-xl border border-primary/20 bg-primary/10 p-4">
              <Info className="h-5 w-5 shrink-0 text-primary" />
              <div>
                <p className="text-sm font-medium text-white">Noch zu wenig Historie</p>
                <p className="mt-0.5 text-sm text-text-secondary">
                  Alle {totalChatters} Chatter wurden erstmalig gesehen. Sobald sie wiederkehren, werden Return-Rate und Stammzuschauer berechnet.
                </p>
              </div>
            </div>
          )}

          <div className={`grid grid-cols-1 items-start justify-items-center gap-4 sm:grid-cols-2 ${CHAT_PENETRATION_ENABLED ? 'xl:grid-cols-6' : 'lg:grid-cols-5'} lg:gap-3`}>
            <LoyaltyGauge label="Neue Zuschauer" percentage={newViewerShare} description="Chatten zum ersten Mal" startColor="var(--color-accent)" endColor="var(--color-primary)" />
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
            <LoyaltyGauge
              label="Aktive Chatters"
              percentage={activeChattersShare}
              valueText={`${activeChattersShare.toFixed(1)}%`}
              description={activeChattersDescription}
              startColor="var(--color-accent)"
              endColor="var(--color-primary)"
            />
            {CHAT_PENETRATION_ENABLED && (
              <LoyaltyGauge
                label="Chat Penetration"
                percentage={chatPenetrationGaugeValue ?? 0}
                valueText={chatPenetrationGaugeValue !== null ? `${chatPenetrationGaugeValue.toFixed(1)}%` : 'N/A'}
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
          </div>
        </div>
      </motion.div>

      {coachingData && !coachingData.empty && <ChatConcentrationSection data={coachingData} />}

      <motion.div initial={{ opacity: 0, y: 20 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: 0.1 }} className="rounded-2xl border border-primary/25 bg-gradient-to-r from-primary/16 via-card to-accent/16 p-6">
        <h3 className="mb-4 font-bold text-white">Chat-Insights</h3>
        <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
          {totalChatters > 0 ? (
            <>
              <InsightItem
                type={chatterReturnRate > 30 ? 'positive' : 'warning'}
                text={
                  chatterReturnRate > 30
                    ? `Starke Community! ${chatterReturnRate.toFixed(0)}% deiner Chatter kommen wieder.`
                    : `${chatterReturnRate.toFixed(0)}% Return Rate - versuche mehr Interaktion!`
                }
              />
              <InsightItem
                type={coreLoyalViewers > 0 ? 'positive' : 'info'}
                text={
                  coreLoyalViewers > 0
                    ? silentCoreLoyalViewers > 0
                      ? `${coreLoyalViewers.toLocaleString('de-DE')} Stammzuschauer erkannt, davon ${silentCoreLoyalViewers.toLocaleString('de-DE')} silent.`
                      : `${coreLoyalViewers.toLocaleString('de-DE')} Stammzuschauer im ${days}-Tage-Fenster erkannt.`
                    : `${returningTrackedViewers.toLocaleString('de-DE')} wiederkehrende Zuschauer, aber noch niemand mit ${loyaltySessionThreshold}+ Streams im Fenster.`
                }
              />
            </>
          ) : (
            <InsightItem type="info" text="Keine aktiven Chatter im Zeitraum: Erst bei echten Chat-Samples werden Treue-Insights angezeigt." />
          )}
        </div>
      </motion.div>

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
        <motion.div initial={{ opacity: 0, y: 20 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: 0.15 }} className="panel-card rounded-2xl p-6">
          <div className="mb-6 flex items-center gap-3">
            <MessageCircle className="h-6 w-6 text-accent" />
            <h2 className="text-xl font-bold text-white">Nachrichtentypen</h2>
          </div>
          <div className="space-y-4">
            {data.messageTypes?.map((type) => (
              <div key={type.type}>
                <div className="mb-1 flex justify-between text-sm">
                  <span className="text-text-secondary">{type.type}</span>
                  <span className="font-medium text-white">{type.percentage}% ({type.count})</span>
                </div>
                <div className="h-2 overflow-hidden rounded-full bg-background/80">
                  <motion.div initial={{ width: 0 }} animate={{ width: `${type.percentage}%` }} transition={{ duration: 1, ease: 'easeOut' }} className="h-full bg-gradient-to-r from-primary to-accent" />
                </div>
              </div>
            ))}
            {(!data.messageTypes || data.messageTypes.length === 0) && (
              <p className="py-4 text-center text-text-secondary">Keine Daten verfügbar</p>
            )}
          </div>
        </motion.div>

        <motion.div initial={{ opacity: 0, y: 20 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: 0.2 }} className="panel-card rounded-2xl p-6">
          <div className="mb-6 flex items-start justify-between gap-4">
            <div>
              <div className="flex items-center gap-3">
                <TrendingUp className="h-6 w-6 text-success" />
                <h2 className="text-xl font-bold text-white">Chat-Nachrichten nach Uhrzeit</h2>
                <span className="rounded-full border border-border/60 px-2 py-0.5 text-[11px] text-text-secondary">{data.timezone || 'UTC'}</span>
              </div>
              <p className="mt-2 text-sm text-text-secondary">Aggregierte Roh-Chat-Nachrichten pro Stunde im gewählten Zeitraum.</p>
            </div>
          </div>
          {hasHourlySamples ? (
            <>
              {hoursWithData < 3 && (
                <div className="mb-3 rounded-lg border border-border/50 bg-background/50 px-3 py-2 text-xs text-text-secondary">
                  Nur {hoursWithData} Stunde{hoursWithData !== 1 ? 'n' : ''} mit Daten — zu wenig für aussagekräftige Tageszeit-Analyse.
                </div>
              )}
              <div className="h-72 rounded-xl border border-border/60 bg-background/35 p-3">
                <ResponsiveContainer width="100%" height="100%">
                  <AreaChart data={hourlyChartData} margin={{ top: 12, right: 8, left: -16, bottom: 0 }}>
                    <defs>
                      <linearGradient id={hourlyChartGradientId} x1="0" y1="0" x2="0" y2="1">
                        <stop offset="5%" stopColor="var(--color-success)" stopOpacity={0.4} />
                        <stop offset="95%" stopColor="var(--color-success)" stopOpacity={0.04} />
                      </linearGradient>
                    </defs>
                    <XAxis dataKey="label" interval={3} tick={{ fontSize: 11, fill: 'var(--color-text-secondary)' }} tickLine={false} axisLine={false} />
                    <YAxis allowDecimals={false} width={42} tick={{ fontSize: 11, fill: 'var(--color-text-secondary)' }} tickLine={false} axisLine={false} />
                    <Tooltip
                      contentStyle={CHART_TOOLTIP_STYLE}
                      labelFormatter={(label) => `${label} Uhr`}
                      formatter={(value) => [`${Number(value).toLocaleString('de-DE')} Nachrichten`, 'Chatvolumen']}
                    />
                    <Area type="monotone" dataKey="count" stroke="var(--color-success)" strokeWidth={3} fill={`url(#${hourlyChartGradientId})`} activeDot={{ r: 5, strokeWidth: 0, fill: 'var(--color-success)' }} />
                  </AreaChart>
                </ResponsiveContainer>
              </div>
              <div className="mt-3 flex flex-wrap items-center justify-between gap-2 text-xs text-text-secondary">
                <span>Jeder Punkt summiert alle Roh-Chat-Nachrichten dieser Stunde.</span>
                <span>Peak bei {peakHour.hour}:00 Uhr mit {peakHour.count.toLocaleString('de-DE')} Nachrichten</span>
              </div>
            </>
          ) : (
            <div className="flex h-64 items-center justify-center rounded-lg border border-border bg-background/50 p-4 text-center text-sm text-text-secondary">
              Keine belastbaren Stundenmuster: zu wenig valide Chat-Timestamps.
            </div>
          )}
        </motion.div>
      </div>

      <motion.div initial={{ opacity: 0, y: 20 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: 0.25 }} className="panel-card rounded-2xl p-6">
        <div className="mb-6 flex items-center gap-3">
          <Award className="h-6 w-6 text-warning" />
          <h2 className="text-xl font-bold text-white">Top Chatter</h2>
        </div>
        {data.topChatters && data.topChatters.length > 0 ? (
          <div className="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-3">
            {data.topChatters.slice(0, 12).map((chatter, index) => (
              <ChatterCard key={chatter.login} rank={index + 1} login={chatter.login} messages={chatter.totalMessages} sessions={chatter.totalSessions} loyaltyScore={chatter.loyaltyScore} />
            ))}
          </div>
        ) : (
          <div className="py-8 text-center text-text-secondary">
            <Users className="mx-auto mb-3 h-12 w-12 opacity-50" />
            <p>Keine Chatter-Daten vorhanden</p>
          </div>
        )}
      </motion.div>

      <motion.div initial={{ opacity: 0, y: 20 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: 0.3 }} className="panel-card rounded-2xl p-6">
        <div className="mb-6 flex items-center gap-3">
          <Users className="h-6 w-6 text-accent" />
          <h2 className="text-xl font-bold text-white">Zuschauer-Profile</h2>
        </div>
        <ViewerProfiles data={viewerProfilesData} />
      </motion.div>

      <PlanGateCard featureId="hype_timeline" title="Hype-Timeline">
        {hypeData && <HypeMomenteSection data={hypeData} selectedSessionId={selectedSessionId} onSessionChange={setSelectedSessionId} />}
      </PlanGateCard>

      <PlanGateCard featureId="chat_content_analysis" title="Chat-Inhaltsanalyse">
        {contentData && <StimmungTopicsSection data={contentData} />}
      </PlanGateCard>

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
      <div className="relative mx-auto mb-3 h-32 w-32">
        <svg className="h-full w-full -rotate-90">
          <circle cx="64" cy="64" r="56" fill="none" stroke="currentColor" strokeWidth="12" className="text-border" />
          <motion.circle cx="64" cy="64" r="56" fill="none" stroke={`url(#${gradientId})`} strokeWidth="12" strokeLinecap="round" strokeDasharray={`${(clampedPercentage / 100) * 352} 352`} initial={{ strokeDasharray: '0 352' }} animate={{ strokeDasharray: `${(clampedPercentage / 100) * 352} 352` }} transition={{ duration: 1, delay: 0.3 }} />
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

function ChatterCard({
  rank,
  login,
  messages,
  sessions,
  loyaltyScore,
}: {
  rank: number;
  login: string;
  messages: number;
  sessions: number;
  loyaltyScore: number;
}) {
  const rankStyle =
    rank === 1
      ? 'bg-gradient-to-br from-yellow-400 to-yellow-600 text-black'
      : rank === 2
        ? 'bg-gradient-to-br from-gray-300 to-gray-500 text-black'
        : rank === 3
          ? 'bg-gradient-to-br from-amber-600 to-amber-800 text-white'
          : 'bg-border text-text-secondary';

  return (
    <motion.div initial={{ opacity: 0, y: 10 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: rank * 0.03 }} className="flex items-center gap-3 rounded-xl border border-border/65 bg-background/75 p-3">
      <div className={`flex h-8 w-8 items-center justify-center rounded-full text-sm font-bold ${rankStyle}`}>{rank}</div>
      <div className="min-w-0 flex-1">
        <div className="truncate font-medium text-white">{login}</div>
        <div className="text-xs text-text-secondary">{messages.toLocaleString('de-DE')} Nachrichten • {sessions} Sessions</div>
      </div>
      <div className="text-right">
        <div className="text-sm font-medium text-primary">{loyaltyScore}</div>
        <div className="text-xs text-text-secondary">Loyalität</div>
      </div>
    </motion.div>
  );
}

function InsightItem({ type, text }: { type: 'positive' | 'warning' | 'info'; text: string }) {
  const styles = {
    positive: 'bg-success/10 border-success/20 text-success',
    warning: 'bg-warning/10 border-warning/20 text-warning',
    info: 'bg-primary/10 border-primary/20 text-primary',
  };

  return (
    <div className={`rounded-lg border p-3 ${styles[type]}`}>
      <p className="text-sm text-white">{text}</p>
    </div>
  );
}
