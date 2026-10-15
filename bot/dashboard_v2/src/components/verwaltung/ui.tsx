import { useT } from '../../context/LanguageContext';
import { type ReactNode } from 'react';
import { motion } from 'framer-motion';
import { Loader2 } from 'lucide-react';
import { EditableCommandName } from './CommandNameSection';
import { SavedIndicator } from './SavedIndicator';
import type { SettingToggleState } from './useSettingToggle';

export interface ToggleProps {
  checked: boolean;
  onChange: () => void;
  label: string;
  disabled?: boolean;
  pending?: boolean;
}

export function Toggle({ checked, onChange, label, disabled = false, pending = false }: ToggleProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled || pending}
      onClick={onChange}
      className={`relative inline-flex h-7 w-12 shrink-0 items-center rounded-full transition-colors ${
        checked ? 'bg-primary' : 'bg-border'
      } disabled:opacity-50 disabled:cursor-not-allowed`}
    >
      <span
        className={`inline-block h-5 w-5 transform rounded-full bg-white transition-[transform,translate,scale] ${
          checked ? 'translate-x-6' : 'translate-x-1'
        }`}
      />
      {pending && (
        <Loader2 aria-hidden="true" className="absolute left-1/2 -translate-x-1/2 h-3.5 w-3.5 animate-spin text-white" />
      )}
    </button>
  );
}

export interface SectionCardProps {
  eyebrow: string;
  title?: string;
  description?: ReactNode;
  icon?: ReactNode;
  danger?: boolean;
  className?: string;
  children: ReactNode;
  attrs?: Record<string, string | number | boolean | undefined>;
}

export function SectionCard({ eyebrow, title, description, icon, danger = false, className = '', children, attrs }: SectionCardProps) {
  return (
    <motion.section
      {...attrs}
      className={`panel-card rounded-2xl p-5 md:p-6 ${danger ? 'border border-danger/30' : ''} ${className}`}
      initial={{ opacity: 0, y: 16 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true }}
      transition={{ duration: 0.32 }}
    >
      <div className={title || description ? 'mb-4' : 'mb-2'}>
        <p className={`flex items-center gap-2 text-sm font-medium uppercase tracking-wider ${danger ? 'text-danger' : 'text-primary'}`}>
          {icon}
          {eyebrow}
        </p>
        {title && <h2 className="display-font mt-1 text-xl font-bold text-white">{title}</h2>}
        {description && <p className="mt-1 text-sm text-text-secondary">{description}</p>}
      </div>
      {children}
    </motion.section>
  );
}

export interface CommandRowProps {
  command?: string;
  title?: ReactNode;
  description?: ReactNode;
  icon?: ReactNode;
  inputSide?: 'start' | 'end';
  toggle?: ToggleProps;
  savedAt?: number | null;
  error?: string | null;
  children?: ReactNode;
}

export function CommandRow({ command, title, description, icon, inputSide = 'start', toggle, savedAt, error, children }: CommandRowProps) {
  const t = useT();
  const input = command
    ? <EditableCommandName command={command} className="w-[140px] shrink-0" />
    : inputSide === 'start'
      ? <span aria-hidden="true" className="hidden w-[140px] shrink-0 sm:block" />
      : null;
  return (
    <div className="border-b border-border/60 py-3 last:border-b-0">
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
        {inputSide === 'start' && input}
        <div className="flex min-w-0 flex-1 basis-48 items-start gap-3">
          {icon}
          <div className="min-w-0">
            {title && <p className="text-sm font-bold text-white">{title}</p>}
            {description && <p className="mt-0.5 text-xs leading-5 text-text-secondary">{description}</p>}
          </div>
        </div>
        <div className="ml-auto flex shrink-0 items-center gap-3">
          {savedAt ? <SavedIndicator key={savedAt} /> : null}
          {inputSide === 'end' && input}
          {toggle && <Toggle {...toggle} />}
        </div>
      </div>
      {error && <p role="alert" className="mt-2 text-xs text-danger">{t(error)}</p>}
      {children && <div className="mt-3 space-y-2 border-l border-border/60 pl-4 sm:ml-[156px]">{children}</div>}
    </div>
  );
}

export interface ToggleCommandRowProps {
  state: SettingToggleState;
  command?: string;
  title: string;
  onText: string;
  offText: string;
  label: string;
  children?: ReactNode;
}

export function ToggleCommandRow({ state, command, title, onText, offText, label, children }: ToggleCommandRowProps) {
  const t = useT();
  const description = state.loading
    ? t('Status wird geladen …')
    : state.enabled === null
      ? t('Einstellung gerade nicht verfügbar.')
      : t(state.enabled ? onText : offText);
  return (
    <CommandRow
      command={command}
      title={t(title)}
      description={description}
      savedAt={state.savedAt}
      error={state.error}
      toggle={{
        checked: Boolean(state.enabled),
        onChange: () => void state.toggle(),
        label: t(label),
        disabled: state.enabled === null,
        pending: state.pending,
      }}
    >
      {state.enabled ? children : null}
    </CommandRow>
  );
}
