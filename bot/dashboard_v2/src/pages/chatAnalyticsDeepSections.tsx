import { useState } from 'react';
import { motion } from 'framer-motion';
import { AtSign, MessageCircle, Smile, Zap, Brain, Loader2, Sparkles, Target } from 'lucide-react';
import {
  Area,
  AreaChart,
  Bar,
  Cell,
  ComposedChart,
  Line,
  Pie,
  PieChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from 'recharts';

import type {
  ChatContentAnalysis,
  ChatHypeTimeline,
  ChatSocialGraph,
  CoachingData,
} from '@/types/analytics';

import { RawChatStatusBanner } from './chatAnalyticsShared';
import { fetchChatMinimaxDeep } from '@/api/ai';

const CHART_TOOLTIP_STYLE = {
  backgroundColor: 'rgba(9, 12, 22, 0.92)',
  border: '1px solid rgba(148, 163, 184, 0.18)',
  borderRadius: 16,
  boxShadow: '0 24px 60px rgba(2, 6, 23, 0.42)',
  color: '#f8fafc',
} as const;

const LOYALTY_LABELS: Record<string, { label: string; color: string }> = {
  oneTimer: { label: 'Einmalig', color: 'bg-error/60' },
  casual: { label: 'Gelegentlich', color: 'bg-warning/60' },
  regular: { label: 'Regulär', color: 'bg-primary/60' },
  loyal: { label: 'Loyal', color: 'bg-success/60' },
};

const TOPIC_COLORS: Record<string, string> = {
  heroes: '#3b82f6',
  builds: '#f59e0b',
  ranked: '#8b5cf6',
  meta: '#ef4444',
  gameplay: '#06b6d4',
  backseat: '#f97316',
  commands: '#60a5fa',
  social: '#06b6d4',
  smalltalk: '#facc15',
  greeting: '#22d3ee',
  community: '#10b981',
  reaction: '#ec4899',
  hype: '#f43f5e',
  feedback: '#84cc16',
  technical: '#f97316',
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
  hype: 'Hype',
  feedback: 'Feedback',
  technical: 'Technik',
  other: 'Sonstiges',
};

function clampPercent(value: number): number {
  if (!Number.isFinite(value)) {
    return 0;
  }
  return Math.max(0, Math.min(100, value));
}

export function ChatConcentrationSection({ data }: { data: CoachingData }) {
  const chat = data.chatConcentration;
  if (!chat || chat.totalChatters === 0) return null;

  const bucketOrder = ['oneTimer', 'casual', 'regular', 'loyal'];
  const buckets = bucketOrder
    .filter((key) => chat.loyaltyBuckets[key])
    .map((key) => ({
      key,
      ...chat.loyaltyBuckets[key],
      ...LOYALTY_LABELS[key],
      widthPct: clampPercent(chat.loyaltyBuckets[key].pct),
    }));

  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ delay: 0.28 }}
      className="panel-card rounded-2xl p-6"
    >
      <div className="mb-6 flex items-center gap-3">
        <MessageCircle className="h-6 w-6 text-primary" />
        <h2 className="text-xl font-bold text-white">Chat-Konzentration</h2>
      </div>

      <div className="mb-6 grid grid-cols-2 gap-4 md:grid-cols-4">
        <div className="rounded-lg bg-background/50 p-3 text-center">
          <div className={`text-2xl font-bold ${chat.top1Pct > 50 ? 'text-error' : chat.top1Pct > 30 ? 'text-warning' : 'text-success'}`}>
            {chat.top1Pct}%
          </div>
          <div className="text-xs text-text-secondary">Top-1 Chatter Anteil</div>
        </div>
        <div className="rounded-lg bg-background/50 p-3 text-center">
          <div className={`text-2xl font-bold ${chat.top3Pct > 70 ? 'text-warning' : 'text-white'}`}>
            {chat.top3Pct}%
          </div>
          <div className="text-xs text-text-secondary">Top-3 kumulativ</div>
        </div>
        <div className="rounded-lg bg-background/50 p-3 text-center">
          <div className="text-2xl font-bold text-white">{chat.msgsPerChatter}</div>
          <div className="text-xs text-text-secondary">Msgs / Chatter</div>
        </div>
        <div className="rounded-lg bg-background/50 p-3 text-center">
          <div className={`text-2xl font-bold ${chat.concentrationIndex > 2500 ? 'text-error' : 'text-white'}`}>
            {chat.concentrationIndex.toLocaleString()}
          </div>
          <div className="text-xs text-text-secondary">HHI-Index</div>
        </div>
      </div>

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
        <div>
          <h3 className="mb-3 text-sm font-medium text-text-secondary">Chatter-Loyalität</h3>
          <div className="mb-3 flex h-8 overflow-hidden rounded-lg">
            {buckets.map((bucket) => (
              <div
                key={bucket.key}
                className={`${bucket.color} flex items-center justify-center text-xs font-medium text-white`}
                style={{ width: `${bucket.widthPct}%`, minWidth: bucket.widthPct > 5 ? undefined : '2px' }}
                title={`${bucket.label}: ${bucket.count} (${bucket.pct}%)`}
              >
                {bucket.pct >= 10 && `${bucket.pct}%`}
              </div>
            ))}
          </div>
          <div className="flex flex-wrap gap-3 text-xs">
            {buckets.map((bucket) => (
              <span key={bucket.key} className="flex items-center gap-1.5 text-text-secondary">
                <span className={`h-2.5 w-2.5 rounded-sm ${bucket.color}`} />
                {bucket.label}: {bucket.count} ({bucket.pct}%)
              </span>
            ))}
          </div>
        </div>

        <div className="rounded-xl border border-border/50 bg-background/30 p-4">
          <div className="mb-3 flex items-center justify-between">
            <h3 className="text-sm font-medium text-text-secondary">Interpretation</h3>
            <span className={`rounded-full px-2 py-0.5 text-xs font-bold ${
              chat.top3Pct > 70 ? 'bg-warning/10 text-warning' : 'bg-success/10 text-success'
            }`}>
              {chat.top3Pct > 70 ? 'hoch konzentriert' : 'ausgeglichen'}
            </span>
          </div>
          <p className="text-sm leading-6 text-text-secondary">
            {chat.top3Pct > 70
              ? 'Ein kleiner Kern dominiert den Chat. Prüfe, ob neue oder stille Zuschauer genug Anlässe zur Beteiligung bekommen.'
              : 'Die Chat-Aktivität verteilt sich relativ breit. Das ist meist robuster und weniger von Einzelpersonen abhängig.'}
          </p>
        </div>
      </div>
    </motion.div>
  );
}

export function HypeMomenteSection({
  data,
  selectedSessionId,
  onSessionChange,
}: {
  data: ChatHypeTimeline;
  selectedSessionId?: number;
  onSessionChange: (id: number | undefined) => void;
}) {
  const correlationLabel =
    Math.abs(data.correlation.chatViewerR) >= 0.4 ? `${data.correlation.chatViewerR.toFixed(2)} r` : 'schwach';

  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ delay: 0.35 }}
      className="panel-card rounded-2xl p-6"
    >
      <RawChatStatusBanner status={data.rawChatStatus} compact />

      <div className="mb-6 flex items-center justify-between">
        <div className="flex items-center gap-3">
          <Zap className="h-6 w-6 text-warning" />
          <h2 className="text-xl font-bold text-white">Hype-Momente</h2>
        </div>
        {data.recentSessions.length > 0 && (
          <select
            className="rounded-lg border border-border bg-background px-3 py-1.5 text-sm text-white focus:border-primary/50 focus:outline-none"
            value={selectedSessionId ?? data.sessionId}
            onChange={(event) => {
              const nextSessionId = Number(event.target.value);
              if (!Number.isFinite(nextSessionId)) {
                onSessionChange(undefined);
                return;
              }
              onSessionChange(
                nextSessionId === data.sessionId && !selectedSessionId ? undefined : nextSessionId
              );
            }}
          >
            <option value={data.sessionId}>
              Aktuelle Session — {data.sessionTitle || data.startedAt.split('T')[0]}
            </option>
            {data.recentSessions.map((session) => (
              <option key={session.id} value={session.id}>
                {session.date} — {session.title || `Session #${session.id}`} (Ø {session.avgMPM} MPM)
              </option>
            ))}
          </select>
        )}
      </div>

      <div className="mb-6 grid grid-cols-2 gap-4 md:grid-cols-4">
        <div className="rounded-lg bg-background/50 p-3 text-center">
          <div className="text-2xl font-bold text-white">{data.avgMPM}</div>
          <div className="text-xs text-text-secondary">Ø Messages/Min</div>
        </div>
        <div className="rounded-lg bg-background/50 p-3 text-center">
          <div className="text-2xl font-bold text-warning">{data.peakMPM}</div>
          <div className="text-xs text-text-secondary">Peak MPM</div>
        </div>
        <div className="rounded-lg bg-background/50 p-3 text-center">
          <div className="text-2xl font-bold text-accent">{data.spikes.length}</div>
          <div className="text-xs text-text-secondary">Hype-Spikes</div>
        </div>
        <div className="rounded-lg bg-background/50 p-3 text-center">
          <div className={`text-sm font-bold ${Math.abs(data.correlation.chatViewerR) >= 0.4 ? 'text-success' : 'text-text-secondary'}`}>
            {correlationLabel}
          </div>
          <div className="text-xs text-text-secondary">Chat↔Viewer Korrelation</div>
        </div>
      </div>

      {data.timeline.length > 0 && (
        <div className="mb-4 h-[280px]">
          <ResponsiveContainer width="100%" height="100%">
            <ComposedChart data={data.timeline}>
              <XAxis dataKey="minute" tickFormatter={(value) => `${value}m`} tick={{ fontSize: 11, fill: 'var(--color-text-secondary)' }} />
              <YAxis yAxisId="left" tick={{ fontSize: 11, fill: 'var(--color-text-secondary)' }} />
              <YAxis yAxisId="right" orientation="right" tick={{ fontSize: 11, fill: 'var(--color-text-secondary)' }} />
              <Tooltip contentStyle={CHART_TOOLTIP_STYLE} labelFormatter={(value) => `Minute ${value}`} />
              <Bar yAxisId="left" dataKey="messages" fill="var(--color-primary)" opacity={0.7} radius={[2, 2, 0, 0]} name="Messages" />
              <Line yAxisId="right" type="monotone" dataKey="viewers" stroke="var(--color-accent)" strokeWidth={2} dot={false} name="Viewer" />
            </ComposedChart>
          </ResponsiveContainer>
        </div>
      )}

      {data.spikes.length > 0 && (
        <div>
          <h3 className="mb-2 text-sm font-medium text-text-secondary">Top Hype-Spikes</h3>
          <div className="space-y-1.5">
            {data.spikes.slice(0, 5).map((spike, index) => (
              <div key={index} className="flex items-center justify-between rounded-lg bg-background/50 px-3 py-2 text-xs">
                <span className="font-medium text-white">Minute {spike.minute}</span>
                <span className="font-bold text-warning">{spike.messages} Messages</span>
                <span className="text-text-secondary">{spike.multiplier}x Durchschnitt</span>
              </div>
            ))}
          </div>
        </div>
      )}
    </motion.div>
  );
}

export function StimmungTopicsSection({ data }: { data: ChatContentAnalysis }) {
  const sentimentColor =
    data.overallSentiment.score > 0.2
      ? 'text-success'
      : data.overallSentiment.score < -0.2
        ? 'text-error'
        : 'text-text-secondary';
  const trendArrow =
    data.overallSentiment.trend === 'rising'
      ? '↑'
      : data.overallSentiment.trend === 'falling'
        ? '↓'
        : '→';
  const topicEntries = Object.entries(data.topicBreakdown).filter(([, value]) => value > 0);
  const topicTotal = topicEntries.reduce((sum, [, value]) => sum + value, 0);
  const donutData = topicEntries.map(([key, value]) => ({
    name: TOPIC_LABELS[key] || key,
    value,
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

      <div className="mb-6 flex items-center gap-3">
        <Smile className="h-6 w-6 text-success" />
        <h2 className="text-xl font-bold text-white">Stimmung & Topics</h2>
        <span className={`text-sm font-bold ${sentimentColor}`}>
          {data.overallSentiment.label} {trendArrow}
        </span>
      </div>

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
        <div>
          <h3 className="mb-3 text-sm font-medium text-text-secondary">Stimmungsverlauf</h3>
          {data.sentimentTimeline.length > 0 ? (
            <div className="h-[200px]">
              <ResponsiveContainer width="100%" height="100%">
                <AreaChart data={data.sentimentTimeline}>
                  <XAxis dataKey="bucket" hide />
                  <YAxis domain={[-1, 1]} tick={{ fontSize: 10, fill: 'var(--color-text-secondary)' }} />
                  <Tooltip contentStyle={CHART_TOOLTIP_STYLE} labelFormatter={(value) => String(value)} />
                  <Area type="monotone" dataKey="score" stroke="var(--color-success)" fill="var(--color-success)" fillOpacity={0.2} strokeWidth={2} name="Sentiment" />
                </AreaChart>
              </ResponsiveContainer>
            </div>
          ) : (
            <p className="py-8 text-center text-xs text-text-secondary">Keine Sentiment-Daten</p>
          )}
          <div className="mt-2 flex justify-between text-xs text-text-secondary">
            <span>Positiv: {data.overallSentiment.positiveCount}</span>
            <span>Negativ: {data.overallSentiment.negativeCount}</span>
            <span>Analysiert: {data.overallSentiment.totalAnalyzed}</span>
          </div>
        </div>

        <div className="space-y-4">
          {data.heroMentions.length > 0 && (
            <div>
              <h3 className="mb-2 text-sm font-medium text-text-secondary">Hero-Mentions</h3>
              <div className="space-y-2">
                {data.heroMentions.slice(0, 8).map((hero) => (
                  <div key={hero.hero} className="flex items-center gap-2">
                    <span className="w-24 truncate text-xs capitalize text-white">{hero.hero.replace('_', ' ')}</span>
                    <div className="h-2 flex-1 overflow-hidden rounded-full bg-background/80">
                      <div
                        className="h-full rounded-full bg-primary/70"
                        style={{ width: `${clampPercent(hero.pct)}%` }}
                      />
                    </div>
                    <span className="w-16 text-right text-xs text-text-secondary">{hero.count} ({hero.pct}%)</span>
                  </div>
                ))}
              </div>
            </div>
          )}

          {donutData.length > 0 && (
            <div>
              <h3 className="mb-2 text-sm font-medium text-text-secondary">Topic-Verteilung</h3>
              <div className="flex items-center gap-4">
                <div className="h-[120px] w-[120px]">
                  <ResponsiveContainer width="100%" height="100%">
                    <PieChart>
                      <Pie data={donutData} innerRadius={30} outerRadius={50} dataKey="value" stroke="none">
                        {donutData.map((entry, index) => (
                          <Cell key={index} fill={entry.color} />
                        ))}
                      </Pie>
                      <Tooltip contentStyle={CHART_TOOLTIP_STYLE} formatter={(value) => `${Number(value).toLocaleString('de-DE')}`} />
                    </PieChart>
                  </ResponsiveContainer>
                </div>
                <div className="flex flex-wrap gap-2 text-xs">
                  {donutData.map((entry, index) => (
                    <span key={`${entry.name}-${index}`} className="flex items-center gap-1.5 text-text-secondary">
                      <span className="h-2.5 w-2.5 rounded-sm" style={{ backgroundColor: entry.color }} />
                      {entry.name}: {topicTotal > 0 ? Math.round((entry.value / topicTotal) * 100) : 0}%
                    </span>
                  ))}
                </div>
              </div>
            </div>
          )}
        </div>
      </div>

      <div className="mt-6 grid grid-cols-1 gap-6 lg:grid-cols-2">
        <div className="rounded-xl border border-border/50 bg-background/30 p-4">
          <div className="mb-3 flex items-center justify-between">
            <h3 className="text-sm font-medium text-text-secondary">Backseat Gaming</h3>
            <span className={`rounded-full px-2 py-0.5 text-xs font-bold ${
              data.backseat.pct > 10 ? 'bg-warning/10 text-warning' : 'bg-success/10 text-success'
            }`}>
              {data.backseat.pct}%
            </span>
          </div>
          <div className="mb-3 flex items-baseline gap-2">
            <span className="text-2xl font-bold text-white">{data.backseat.count.toLocaleString('de-DE')}</span>
            <span className="text-xs text-text-secondary">Messages mit Coaching-Charakter</span>
          </div>
          {data.backseat.examples.length > 0 && (
            <div className="max-h-[100px] space-y-1 overflow-y-auto">
              {data.backseat.examples.slice(0, 5).map((example, index) => (
                <div key={index} className="truncate rounded bg-background/50 px-2 py-1 text-xs italic text-text-secondary">
                  "{example}"
                </div>
              ))}
            </div>
          )}
        </div>

        <div className="rounded-xl border border-border/50 bg-background/30 p-4">
          <div className="mb-3 flex items-center justify-between">
            <h3 className="text-sm font-medium text-text-secondary">Chat-Tiefe</h3>
            <span className="text-xs text-text-secondary">Ø {data.engagementDepth.avgWordCount} Wörter/Message</span>
          </div>
          <div className="mb-3 flex h-8 overflow-hidden rounded-lg">
            {data.engagementDepth.reactionPct > 0 && (
              <div
                className="flex items-center justify-center bg-warning/60 text-xs font-medium text-white"
                style={{
                  width: `${clampPercent(data.engagementDepth.reactionPct)}%`,
                  minWidth: clampPercent(data.engagementDepth.reactionPct) > 8 ? undefined : '2px',
                }}
              >
                {data.engagementDepth.reactionPct >= 10 && `${data.engagementDepth.reactionPct}%`}
              </div>
            )}
            {data.engagementDepth.shortPct > 0 && (
              <div
                className="flex items-center justify-center bg-primary/60 text-xs font-medium text-white"
                style={{
                  width: `${clampPercent(data.engagementDepth.shortPct)}%`,
                  minWidth: clampPercent(data.engagementDepth.shortPct) > 8 ? undefined : '2px',
                }}
              >
                {data.engagementDepth.shortPct >= 10 && `${data.engagementDepth.shortPct}%`}
              </div>
            )}
            {data.engagementDepth.discussionPct > 0 && (
              <div
                className="flex items-center justify-center bg-success/60 text-xs font-medium text-white"
                style={{
                  width: `${clampPercent(data.engagementDepth.discussionPct)}%`,
                  minWidth: clampPercent(data.engagementDepth.discussionPct) > 8 ? undefined : '2px',
                }}
              >
                {data.engagementDepth.discussionPct >= 10 && `${data.engagementDepth.discussionPct}%`}
              </div>
            )}
          </div>
          <div className="flex flex-wrap gap-3 text-xs">
            <span className="flex items-center gap-1.5 text-text-secondary">
              <span className="h-2.5 w-2.5 rounded-sm bg-warning/60" />
              Reactions (1-3): {data.engagementDepth.reaction.toLocaleString('de-DE')} ({data.engagementDepth.reactionPct}%)
            </span>
            <span className="flex items-center gap-1.5 text-text-secondary">
              <span className="h-2.5 w-2.5 rounded-sm bg-primary/60" />
              Kurz (4-10): {data.engagementDepth.short.toLocaleString('de-DE')} ({data.engagementDepth.shortPct}%)
            </span>
            <span className="flex items-center gap-1.5 text-text-secondary">
              <span className="h-2.5 w-2.5 rounded-sm bg-success/60" />
              Diskussion (11+): {data.engagementDepth.discussion.toLocaleString('de-DE')} ({data.engagementDepth.discussionPct}%)
            </span>
          </div>
        </div>
      </div>
    </motion.div>
  );
}

export function ChatNetzwerkSection({ data }: { data: ChatSocialGraph }) {
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

      <div className="mb-6 flex items-center gap-3">
        <AtSign className="h-6 w-6 text-accent" />
        <h2 className="text-xl font-bold text-white">Chat-Netzwerk</h2>
      </div>

      {data.totalMentions === 0 ? (
        <div className="rounded-xl border border-border/60 bg-background/40 px-4 py-8 text-center text-sm text-text-secondary">
          Keine belastbaren Mention-Daten im gewählten Zeitraum.
        </div>
      ) : (
        <>
          <div className="mb-6 grid grid-cols-3 gap-4">
            <div className="rounded-lg bg-background/50 p-3 text-center">
              <div className="text-2xl font-bold text-white">{data.totalMentions}</div>
              <div className="text-xs text-text-secondary">@Mentions gesamt</div>
            </div>
            <div className="rounded-lg bg-background/50 p-3 text-center">
              <div className="text-2xl font-bold text-accent">{data.uniqueMentioners}</div>
              <div className="text-xs text-text-secondary">Unique Mentioner</div>
            </div>
            <div className="rounded-lg bg-background/50 p-3 text-center">
              <div className="text-2xl font-bold text-primary">{data.uniqueMentioned}</div>
              <div className="text-xs text-text-secondary">Erwähnte User</div>
            </div>
          </div>

          <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
            <div>
              <h3 className="mb-3 text-sm font-medium text-text-secondary">Conversation-Hubs</h3>
              <div className="space-y-2">
                {data.hubs.slice(0, 5).map((hub, index) => (
                  <div key={hub.login} className="flex items-center gap-3 rounded-xl border border-border/65 bg-background/75 p-3">
                    <div className="flex h-7 w-7 items-center justify-center rounded-full bg-accent/20 text-xs font-bold text-accent">
                      {index + 1}
                    </div>
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-sm font-medium text-white">{hub.login}</div>
                      <div className="text-xs text-text-secondary">
                        {hub.mentionsSent} gesendet · {hub.mentionsReceived} erhalten
                      </div>
                    </div>
                    <div className="text-sm font-bold text-accent">{hub.score}</div>
                  </div>
                ))}
              </div>
            </div>

            <div className="space-y-4">
              <div>
                <h3 className="mb-3 text-sm font-medium text-text-secondary">Top-Gespräche</h3>
                <div className="space-y-1.5">
                  {data.topPairs.slice(0, 8).map((pair, index) => (
                    <div key={index} className="flex items-center justify-between rounded-lg bg-background/50 px-3 py-2 text-xs">
                      <span className="text-white">
                        <span className="font-medium">{pair.from}</span>
                        <span className="mx-1.5 text-text-secondary">→</span>
                        <span className="font-medium">{pair.to}</span>
                      </span>
                      <span className="font-bold text-accent">{pair.count}x</span>
                    </div>
                  ))}
                </div>
              </div>

              <div className="rounded-xl border border-border/50 bg-background/30 p-4">
                <h3 className="mb-3 text-sm font-medium text-text-secondary">Mention-Verteilung</h3>
                <div className="space-y-2 text-sm text-text-secondary">
                  <div className="flex items-center justify-between">
                    <span>Einmal erwähnt</span>
                    <span className="font-medium text-white">{data.mentionDistribution.mentionedOnce}</span>
                  </div>
                  <div className="flex items-center justify-between">
                    <span>2 bis 5 Erwähnungen</span>
                    <span className="font-medium text-white">{data.mentionDistribution.mentioned2to5}</span>
                  </div>
                  <div className="flex items-center justify-between">
                    <span>Mehr als 5 Erwähnungen</span>
                    <span className="font-medium text-white">{data.mentionDistribution.mentioned5plus}</span>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </>
      )}
    </motion.div>
  );
}

export function ChatMinimaxDeepSection({
  streamer,
  sessionId,
}: {
  streamer: string;
  sessionId?: number;
}) {
  const [data, setData] = useState<any>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleAnalysis = async () => {
    if (!sessionId) return;
    setIsLoading(true);
    setError(null);
    try {
      const res = await fetchChatMinimaxDeep(streamer, sessionId);
      setData(res);
    } catch (err: any) {
      setError(err.message || 'Analyse fehlgeschlagen');
    } finally {
      setIsLoading(false);
    }
  };

  if (!sessionId) return null;

  const topicEntries = data ? Object.entries(data.category_counts).filter(([, v]) => (v as number) > 0) : [];
  const topicTotal = topicEntries.reduce((sum, [, v]) => sum + (v as number), 0);
  const donutData = topicEntries.map(([key, value]) => ({
    name: TOPIC_LABELS[key] || key,
    value: value as number,
    color: TOPIC_COLORS[key.toLowerCase()] || '#6b7280',
  }));

  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ delay: 0.5 }}
      className="panel-card rounded-2xl p-6 bg-gradient-to-br from-background via-background to-primary/5"
    >
      <div className="mb-6 flex items-center justify-between">
        <div className="flex items-center gap-3">
          <Brain className="h-6 w-6 text-primary" />
          <h2 className="text-xl font-bold text-white">MiniMax Chat-Analyse</h2>
          <span className="rounded-full bg-primary/10 px-2 py-0.5 text-[10px] font-bold uppercase tracking-wider text-primary border border-primary/20">
            KI-Powered
          </span>
        </div>
        {!data && !isLoading && (
          <button
            onClick={handleAnalysis}
            className="flex items-center gap-2 rounded-xl bg-primary px-4 py-2 text-sm font-bold text-white shadow-lg shadow-primary/20 transition-all hover:scale-105 active:scale-95"
          >
            <Sparkles className="h-4 w-4" />
            Analyse starten
          </button>
        )}
      </div>

      {isLoading && (
        <div className="flex flex-col items-center justify-center py-12">
          <Loader2 className="h-12 w-12 animate-spin text-primary opacity-50" />
          <p className="mt-4 text-sm text-text-secondary">MiniMax analysiert die Nachrichten-Substanz...</p>
        </div>
      )}

      {error && (
        <div className="rounded-xl border border-error/20 bg-error/10 p-4 text-sm text-error">
          {error}
        </div>
      )}

      {data && (
        <div className="grid grid-cols-1 gap-8 lg:grid-cols-2">
          <div className="space-y-6">
            <div>
              <div className="mb-4 flex items-center justify-between">
                <h3 className="text-sm font-medium text-text-secondary">Chat-Tiefe (AI Score)</h3>
                <span className="text-2xl font-bold text-primary">{data.chat_depth_score}%</span>
              </div>
              <div className="h-3 w-full overflow-hidden rounded-full bg-background/80 border border-border/50">
                <motion.div
                  initial={{ width: 0 }}
                  animate={{ width: `${data.chat_depth_score}%` }}
                  className="h-full bg-gradient-to-r from-primary/60 to-primary"
                />
              </div>
              <p className="mt-4 text-sm leading-relaxed text-text-secondary italic">
                "{data.chat_depth_explanation}"
              </p>
            </div>

            <div>
              <h3 className="mb-3 text-sm font-medium text-text-secondary">Top Themen</h3>
              <div className="flex flex-wrap gap-2">
                {data.top_topics.map((topic: string, i: number) => (
                  <span key={i} className="flex items-center gap-1.5 rounded-lg border border-primary/20 bg-primary/5 px-3 py-1.5 text-xs text-primary-light">
                    <Target className="h-3 w-3" />
                    {topic}
                  </span>
                ))}
              </div>
            </div>
          </div>

          <div>
            <h3 className="mb-4 text-sm font-medium text-text-secondary">KI-Kategorisierung</h3>
            <div className="flex items-center gap-6">
              <div className="h-[160px] w-[160px]">
                <ResponsiveContainer width="100%" height="100%">
                  <PieChart>
                    <Pie data={donutData} innerRadius={45} outerRadius={65} dataKey="value" stroke="none">
                      {donutData.map((entry, index) => (
                        <Cell key={index} fill={entry.color} />
                      ))}
                    </Pie>
                    <Tooltip contentStyle={CHART_TOOLTIP_STYLE} />
                  </PieChart>
                </ResponsiveContainer>
              </div>
              <div className="grid grid-cols-1 gap-x-4 gap-y-2 text-xs">
                {donutData.map((entry, index) => (
                  <span key={index} className="flex items-center gap-2 text-text-secondary">
                    <span className="h-2.5 w-2.5 rounded-sm" style={{ backgroundColor: entry.color }} />
                    <span className="font-medium text-white">{entry.name}:</span> {Math.round((entry.value / topicTotal) * 100)}%
                  </span>
                ))}
              </div>
            </div>
          </div>
        </div>
      )}

      {!data && !isLoading && !error && (
        <p className="text-center text-sm text-text-secondary opacity-60">
          Klicke auf "Analyse starten", um eine detaillierte KI-Auswertung der Chat-Substanz für diese Session zu erhalten.
        </p>
      )}
    </motion.div>
  );
}

