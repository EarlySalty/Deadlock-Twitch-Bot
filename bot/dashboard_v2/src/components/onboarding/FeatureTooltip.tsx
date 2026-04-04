import { motion } from 'framer-motion';
import type { LucideIcon } from 'lucide-react';

interface FeatureTooltipProps {
  title: string;
  description: string;
  icon?: LucideIcon;
  position?: 'top' | 'bottom' | 'left' | 'right';
  children?: React.ReactNode;
}

export function FeatureTooltip({
  title,
  description,
  icon: Icon,
  position = 'top',
  children,
}: FeatureTooltipProps) {
  const arrowClass = {
    top: 'top-full left-1/2 -translate-x-1/2 border-t-primary border-l-transparent border-r-transparent border-b-transparent',
    bottom: 'bottom-full left-1/2 -translate-x-1/2 border-b-primary border-l-transparent border-r-transparent border-t-transparent',
    left: 'left-full top-1/2 -translate-y-1/2 border-l-primary border-t-transparent border-b-transparent border-r-transparent',
    right: 'right-full top-1/2 -translate-y-1/2 border-r-primary border-t-transparent border-b-transparent border-l-transparent',
  }[position];

  return (
    <div className="relative inline-block">
      {children}
      <motion.div
        initial={{ opacity: 0, scale: 0.95 }}
        animate={{ opacity: 1, scale: 1 }}
        exit={{ opacity: 0, scale: 0.95 }}
        transition={{ duration: 0.2 }}
        className="absolute z-50 w-64 p-4 panel-card rounded-xl shadow-xl"
        style={{
          [position === 'top' || position === 'bottom' ? 'top' : 'left']: '100%',
          [position === 'top' ? 'marginTop' : position === 'bottom' ? 'marginBottom' : position === 'left' ? 'marginLeft' : 'marginRight']: '8px',
        }}
      >
        <div
          className={`absolute w-0 h-0 border-8 border-solid ${arrowClass}`}
          style={{ [position === 'top' || position === 'bottom' ? 'top' : 'left']: '-8px' }}
        />
        {Icon && (
          <div className="w-8 h-8 rounded-lg gradient-accent flex items-center justify-center mb-3">
            <Icon className="w-4 h-4 text-white" />
          </div>
        )}
        <h3 className="text-sm font-bold text-white mb-1">{title}</h3>
        <p className="text-xs text-text-secondary leading-relaxed">{description}</p>
      </motion.div>
    </div>
  );
}
