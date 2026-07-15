import { motion } from 'framer-motion';

interface OrbConfig {
  size: string;
  color: string;
  position: { top?: string; bottom?: string; left?: string; right?: string };
  animation: {
    x: number[];
    y: number[];
    duration: number;
  };
}

const ORBS: OrbConfig[] = [
  {
    size: 'w-[600px] h-[600px]',
    color: 'rgba(201,168,106,0.3)',
    position: { top: '-200px', right: '-100px' },
    animation: {
      x: [0, 30, -20, 0],
      y: [0, -40, 20, 0],
      duration: 20,
    },
  },
  {
    size: 'w-[500px] h-[500px]',
    color: 'rgba(85, 151, 143, 0.25)',
    position: { bottom: '20%', left: '-150px' },
    animation: {
      x: [0, -25, 35, 0],
      y: [0, 30, -35, 0],
      duration: 25,
    },
  },
  {
    size: 'w-[400px] h-[400px]',
    color: 'rgba(201,168,106,0.18)',
    position: { top: '60%', right: '10%' },
    animation: {
      x: [0, 20, -30, 10, 0],
      y: [0, -20, 40, -10, 0],
      duration: 30,
    },
  },
];

export function GlowOrb() {
  return (
    <div
      className="fixed inset-0 z-0 overflow-hidden pointer-events-none"
      aria-hidden="true"
      data-glow-orb
    >
      {ORBS.map((orb, i) => (
        <motion.div
          key={i}
          className={`absolute rounded-full blur-[120px] opacity-20 ${orb.size}`}
          style={{
            background: orb.color,
            ...orb.position,
          }}
          animate={{
            x: orb.animation.x,
            y: orb.animation.y,
          }}
          transition={{
            duration: orb.animation.duration,
            repeat: Infinity,
            ease: 'linear',
          }}
        />
      ))}
    </div>
  );
}
