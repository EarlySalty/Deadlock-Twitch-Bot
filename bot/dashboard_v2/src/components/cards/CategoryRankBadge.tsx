import { motion } from 'framer-motion';
import { Trophy } from 'lucide-react';

interface CategoryRankBadgeProps {
  rank: number;
  total: number;
}

export function CategoryRankBadge({ rank, total }: CategoryRankBadgeProps) {
  const percentile = total > 0 ? Math.round(((total - rank) / total) * 100) : 0;

  // Color based on ranking
  const getRankColor = () => {
    if (rank <= 3) return 'from-primary-hover to-primary';
    if (rank <= 10) return 'from-primary to-accent';
    if (percentile >= 50) return 'from-accent to-accent-hover';
    return 'from-secondary to-card-hover';
  };

  return (
    <motion.div
      initial={{ opacity: 0, scale: 0.9 }}
      animate={{ opacity: 1, scale: 1 }}
      className="bg-card rounded-xl border border-border p-4 flex items-center gap-4"
    >
      <div className={`w-12 h-12 rounded-full bg-gradient-to-br ${getRankColor()} flex items-center justify-center shrink-0`}>
        <Trophy className="w-6 h-6 text-[#16100a]" />
      </div>
      <div className="min-w-0">
        <div className="text-2xl font-bold text-transparent bg-gradient-to-r from-primary to-accent bg-clip-text">
          Platz {rank}
        </div>
        <div className="text-sm text-text-secondary">
          von {total} Deadlock-Streamern (Top {100 - percentile}%)
        </div>
      </div>
    </motion.div>
  );
}
