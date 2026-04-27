import { motion } from 'framer-motion';
import { TrendingUp, TrendingDown, Minus } from 'lucide-react';
import type { LucideIcon } from 'lucide-react';

interface KpiCardProps {
  title: string;
  value: string | number;
  subValue?: string;
  trend?: number;
  icon?: LucideIcon;
  color?: 'blue' | 'green' | 'purple' | 'yellow' | 'red';
  size?: 'normal' | 'large';
}

const colorClasses = {
  blue: 'bg-primary/15 text-primary border border-primary/25',
  green: 'bg-success/15 text-success border border-success/25',
  purple: 'bg-accent/15 text-accent border border-accent/25',
  yellow: 'bg-warning/15 text-warning border border-warning/25',
  red: 'bg-danger/15 text-danger border border-danger/25',
};

export function KpiCard({
  title,
  value,
  subValue,
  trend,
  icon: Icon,
  color = 'blue',
  size = 'normal',
}: KpiCardProps) {
  const TrendIcon = trend && trend > 0 ? TrendingUp : trend && trend < 0 ? TrendingDown : Minus;
  const trendColor =
    trend && trend > 0
      ? 'text-success'
      : trend && trend < 0
      ? 'text-danger'
      : 'text-text-secondary';

  const topGlow = {
    blue: 'from-primary/45',
    green: 'from-success/45',
    purple: 'from-accent/45',
    yellow: 'from-warning/45',
    red: 'from-danger/45',
  };

  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      className={`panel-card soft-elevate p-5 rounded-2xl ${
        size === 'large' ? 'md:col-span-2' : ''
      }`}
    >
      <div className={`absolute inset-x-6 top-0 h-px bg-gradient-to-r ${topGlow[color]} via-white/20 to-transparent`} />
      <div className="flex justify-between items-start mb-2">
        <span className="text-text-secondary text-[11px] font-semibold uppercase tracking-[0.14em]">
          {title}
        </span>
        {Icon && (
          <div className={`p-2.5 rounded-xl ${colorClasses[color]}`}>
            <Icon className="w-4 h-4" />
          </div>
        )}
      </div>

      <div className="flex items-end gap-3 mt-1">
        <span className={`display-font font-bold text-white ${size === 'large' ? 'text-4xl' : 'text-3xl'}`}>
          {value}
        </span>
        {trend != null && (
          <span className={`text-sm font-medium mb-1 flex items-center gap-1 ${trendColor}`}>
            <TrendIcon className="w-3 h-3" />
            {Math.abs(trend).toFixed(1)}%
          </span>
        )}
      </div>

      {subValue && (
        <div className="mt-3 text-xs text-text-secondary font-medium">{subValue}</div>
      )}
    </motion.div>
  );
}
