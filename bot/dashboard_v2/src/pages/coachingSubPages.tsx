import { useMemo } from 'react';
import { motion } from 'framer-motion';
import { GraduationCap, AlertCircle, Loader2, Zap, Target } from 'lucide-react';
import { useCoaching } from '@/hooks/useAnalytics';
import {
  RecommendationCard,
  EfficiencySection,
  DurationSection,
  ScheduleSection,
  TitleSection,
  TagSection,
  RetentionSection,
  CommunitySection,
  DoubleStreamSection,
  RaidNetworkSection,
  PeerComparisonSection,
  CompetitionDensitySection,
} from '@/pages/Coaching';
import type { TimeRange } from '@/types/analytics';

interface CoachingSubPageProps {
  streamer: string;
  days: TimeRange;
}

export function CoachingEmpfehlungen({ streamer, days }: CoachingSubPageProps) {
  const { data, isLoading } = useCoaching(streamer, days);

  const { topRecs, otherRecs } = useMemo(() => {
    if (!data?.recommendations) return { topRecs: [], otherRecs: [] };
    const top = data.recommendations.filter(r => r.priority === 'critical' || r.priority === 'high');
    const other = data.recommendations.filter(r => r.priority === 'medium' || r.priority === 'low');
    return { topRecs: top, otherRecs: other };
  }, [data]);

  if (!streamer) {
    return (
      <div className="flex flex-col items-center justify-center h-64">
        <AlertCircle className="w-12 h-12 text-text-secondary mb-4" />
        <p className="text-text-secondary text-lg">Waehle einen Streamer aus</p>
      </div>
    );
  }

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-64">
        <Loader2 className="w-8 h-8 animate-spin text-primary" />
      </div>
    );
  }

  if (!data || data.empty) {
    return (
      <div className="flex flex-col items-center justify-center h-64">
        <GraduationCap className="w-12 h-12 text-text-secondary mb-4" />
        <p className="text-text-secondary text-lg">Keine Daten fuer Coaching-Analyse</p>
        <p className="text-text-secondary text-sm mt-2">Streame mehr, um personalisierte Empfehlungen zu erhalten!</p>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      {topRecs.length > 0 && (
        <motion.div initial={{ opacity: 0, y: 20 }} animate={{ opacity: 1, y: 0 }}>
          <div className="flex items-center gap-3 mb-4">
            <Zap className="w-6 h-6 text-warning" />
            <h2 className="text-xl font-bold text-white">Top-Empfehlungen</h2>
          </div>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            {topRecs.map((rec, i) => (
              <RecommendationCard key={i} rec={rec} index={i} />
            ))}
          </div>
        </motion.div>
      )}

      {otherRecs.length > 0 && (
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.5 }}
        >
          <div className="flex items-center gap-3 mb-4">
            <Target className="w-6 h-6 text-primary" />
            <h2 className="text-xl font-bold text-white">Weitere Empfehlungen</h2>
          </div>
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
            {otherRecs.map((rec, i) => (
              <RecommendationCard key={i} rec={rec} index={i} />
            ))}
          </div>
        </motion.div>
      )}
    </div>
  );
}

export function CoachingFormat({ streamer, days }: CoachingSubPageProps) {
  const { data, isLoading } = useCoaching(streamer, days);

  if (!streamer) {
    return (
      <div className="flex flex-col items-center justify-center h-64">
        <AlertCircle className="w-12 h-12 text-text-secondary mb-4" />
        <p className="text-text-secondary text-lg">Waehle einen Streamer aus</p>
      </div>
    );
  }

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-64">
        <Loader2 className="w-8 h-8 animate-spin text-primary" />
      </div>
    );
  }

  if (!data || data.empty) {
    return (
      <div className="flex flex-col items-center justify-center h-64">
        <GraduationCap className="w-12 h-12 text-text-secondary mb-4" />
        <p className="text-text-secondary text-lg">Keine Daten fuer Coaching-Analyse</p>
        <p className="text-text-secondary text-sm mt-2">Streame mehr, um personalisierte Empfehlungen zu erhalten!</p>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <EfficiencySection data={data} />
      <DurationSection data={data} />
      <ScheduleSection data={data} />
      <TitleSection data={data} />
      <TagSection data={data} />
    </div>
  );
}

export function CoachingCommunity({ streamer, days }: CoachingSubPageProps) {
  const { data, isLoading } = useCoaching(streamer, days);

  if (!streamer) {
    return (
      <div className="flex flex-col items-center justify-center h-64">
        <AlertCircle className="w-12 h-12 text-text-secondary mb-4" />
        <p className="text-text-secondary text-lg">Waehle einen Streamer aus</p>
      </div>
    );
  }

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-64">
        <Loader2 className="w-8 h-8 animate-spin text-primary" />
      </div>
    );
  }

  if (!data || data.empty) {
    return (
      <div className="flex flex-col items-center justify-center h-64">
        <GraduationCap className="w-12 h-12 text-text-secondary mb-4" />
        <p className="text-text-secondary text-lg">Keine Daten fuer Coaching-Analyse</p>
        <p className="text-text-secondary text-sm mt-2">Streame mehr, um personalisierte Empfehlungen zu erhalten!</p>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <RetentionSection data={data} />
      <CommunitySection data={data} />
      {data.doubleStreamDetection?.detected && (
        <DoubleStreamSection data={data} />
      )}
      <RaidNetworkSection data={data} />
      <PeerComparisonSection data={data} />
      <CompetitionDensitySection data={data} />
    </div>
  );
}
