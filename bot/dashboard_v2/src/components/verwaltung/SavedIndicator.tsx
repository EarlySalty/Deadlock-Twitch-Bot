import { useT } from '../../context/LanguageContext';
import { useEffect, useState } from 'react';

export function SavedIndicator({ className = '' }: { className?: string }) {
  const t = useT();
  const [visible, setVisible] = useState(true);
  useEffect(() => {
    const timer = window.setTimeout(() => setVisible(false), 2500);
    return () => window.clearTimeout(timer);
  }, []);
  return (
    <span role="status" className={`text-xs text-success transition-opacity ${visible ? 'opacity-100' : 'opacity-0'} ${className}`}>
      {visible ? t('Gespeichert ✓') : ''}
    </span>
  );
}
