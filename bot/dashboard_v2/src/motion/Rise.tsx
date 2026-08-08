import type { CSSProperties, ElementType, ReactNode } from 'react';
import { riseStyle, type RiseSeconds } from './rise';

type RiseProps = {
  children: ReactNode;
  /** Index in der Reihe (0 = erster Block) oder der uebernommene Sekundenwert. */
  step?: number | RiseSeconds;
  className?: string;
  style?: CSSProperties;
  as?: ElementType;
} & Record<string, unknown>;

/**
 * Auftritt eines Abschnitts oder einer Kachel.
 *
 * Ersetzt `<motion.div initial={{opacity:0,y:20}} animate={{opacity:1,y:0}}>`:
 * gleiche Wirkung, aber als CSS-Animation. Der Unterschied ist nicht
 * kosmetisch — framer-motions `y`-Prop laeuft ueber den Hauptthread und
 * verliert Bilder, waehrend das Dashboard seine Abfragen nachlaedt, also
 * genau in dem Moment, in dem der Auftritt stattfindet.
 *
 * Bewegung und Verzoegerung stehen zentral: `.rise-in` in
 * `bot/shared-theme/motion.css`, die Staffelung in `./rise.ts`.
 */
export function Rise({ children, step = 0, className, style, as, ...rest }: RiseProps) {
  const Tag = (as ?? 'div') as ElementType;
  const delayStyle = riseStyle(step);
  const merged =
    delayStyle || style ? ({ ...delayStyle, ...style } as CSSProperties) : undefined;

  return (
    <Tag className={className ? `rise-in ${className}` : 'rise-in'} style={merged} {...rest}>
      {children}
    </Tag>
  );
}
