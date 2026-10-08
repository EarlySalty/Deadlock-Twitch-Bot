import { useState, type RefObject } from 'react';
import { Maximize2 } from 'lucide-react';
import { useT } from '@/context/LanguageContext';

type SafariVideo = HTMLVideoElement & { webkitEnterFullscreen?: () => void };

export function VideoFullscreenButton({ videoRef, className = '' }: {
  videoRef: RefObject<HTMLVideoElement | null>;
  className?: string;
}) {
  const t = useT();
  const [failed, setFailed] = useState(false);
  const enterFullscreen = async () => {
    const video = videoRef.current as SafariVideo | null;
    if (!video) return;
    setFailed(false);
    if (typeof video.requestFullscreen === 'function') {
      try {
        await video.requestFullscreen();
        return;
      } catch {
        if (typeof video.webkitEnterFullscreen !== 'function') {
          setFailed(true);
          return;
        }
      }
    }
    try {
      if (typeof video.webkitEnterFullscreen === 'function') {
        video.webkitEnterFullscreen();
      } else {
        setFailed(true);
      }
    } catch {
      setFailed(true);
    }
  };
  return (
    <div className={className}>
      <button type="button" className="studio-button" onClick={() => void enterFullscreen()}>
        <Maximize2 className="h-4 w-4" aria-hidden="true" />
        {t('Vollbild')}
      </button>
      {failed && <p role="alert" className="mt-2 text-sm text-danger">{t('Vollbild konnte nicht geöffnet werden. Nutze die Videosteuerung oder versuche es erneut.')}</p>}
    </div>
  );
}
