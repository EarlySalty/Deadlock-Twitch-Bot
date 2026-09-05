import { useState, type AnimationEvent, type CSSProperties, type ElementType, type ReactNode } from 'react';
import { riseStyle, type RiseSeconds } from './rise';

type RiseProps = {
  children: ReactNode;
  step?: number | RiseSeconds;
  className?: string;
  style?: CSSProperties;
  as?: ElementType;
} & Record<string, unknown>;

export function Rise({ children, step = 0, className, style, as, ...rest }: RiseProps) {
  const Tag = (as ?? 'div') as ElementType;
  const [risen, setRisen] = useState(false);
  const delayStyle = riseStyle(step);
  const merged =
    delayStyle || style ? ({ ...delayStyle, ...style } as CSSProperties) : undefined;

  const base = risen ? '' : 'rise-in';
  const classes = className ? (base ? `${base} ${className}` : className) : base || undefined;

  const handleAnimationEnd = (event: AnimationEvent<HTMLElement>) => {
    if (event.target === event.currentTarget && event.animationName === 'ddc-rise-in') {
      setRisen(true);
    }
  };

  return (
    <Tag
      className={classes}
      style={merged}
      {...rest}
      onAnimationEnd={handleAnimationEnd}
    >
      {children}
    </Tag>
  );
}
