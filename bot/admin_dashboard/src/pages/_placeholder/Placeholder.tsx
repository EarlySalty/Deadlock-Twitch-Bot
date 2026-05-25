import { PageHeader } from '@/components/layout/PageHeader';

interface PlaceholderProps {
  title: string;
  description?: string;
  plannedStep?: number;
}

export function Placeholder({ title, description, plannedStep }: PlaceholderProps) {
  const resolvedDescription =
    description || 'Diese Fläche wird im Re-Build als eigener Schritt ergänzt. Die Informationsarchitektur und das Routing stehen bereits.';

  return (
    <div className="space-y-5">
      <PageHeader
        title={title}
        description={resolvedDescription}
        secondaryChips={
          plannedStep ? <span className="stat-pill">Geplant für Schritt {plannedStep}</span> : <span className="stat-pill">Shell aktiv</span>
        }
      />

      <section className="panel-card rounded-[1.8rem] p-6">
        <div className="max-w-3xl">
          <p className="text-xs font-semibold uppercase tracking-[0.28em] text-text-secondary">Placeholder</p>
          <p className="mt-3 text-sm leading-6 text-text-secondary">
            Seite ist bewusst noch ohne Fachlogik. In diesem Schritt wurde nur die neue Navigationsstruktur und Page-Shell vorbereitet.
          </p>
        </div>
      </section>
    </div>
  );
}
