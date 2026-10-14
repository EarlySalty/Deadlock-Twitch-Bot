import type { ComponentType, ReactNode } from 'react';

export function StudioSectionHeading({
  title,
  description,
  icon: Icon,
  actions,
  id,
  level = 2,
}: {
  title: string;
  description?: string;
  icon?: ComponentType<{ className?: string; 'aria-hidden'?: boolean }>;
  actions?: ReactNode;
  id?: string;
  level?: 2 | 3;
}) {
  const Heading = level === 3 ? 'h3' : 'h2';
  return (
    <header className="studio-section-header">
      <div className="studio-section-copy">
        <Heading id={id} className="studio-section-title">
          {Icon && <Icon className="h-4 w-4" aria-hidden />}
          {title}
        </Heading>
        {description && <p>{description}</p>}
      </div>
      {actions && <div className="studio-section-actions">{actions}</div>}
    </header>
  );
}
