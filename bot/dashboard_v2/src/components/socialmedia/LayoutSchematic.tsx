import { useT } from '@/context/LanguageContext';
import type { LayoutPayload } from '@/types/socialMedia';

export function LayoutSchematic({ mode }: { mode: LayoutPayload['mode'] }) {
  const t = useT();
  const stacked = mode === 'stacked';
  const pip = mode === 'pip';
  if (mode === 'fill') {
    return (
      <svg className="studio-layout-schematic" viewBox="0 0 108 192" aria-hidden="true">
        <rect className="studio-layout-background" fill="#161616" stroke="#362c23" x="1" y="1" width="106" height="190" rx="7" />
        <rect className="studio-layout-game" fill="#2a221c" stroke="#7a6c57" x="6" y="6" width="96" height="160" rx="3" />
        <path className="studio-layout-scene" fill="#3a2e25" d="M6 150 28 92 48 118 74 70 102 150Z" />
        <circle className="studio-layout-crosshair" fill="none" stroke="#f2eee6" strokeWidth="0.8" cx="54" cy="86" r="7" />
        <path className="studio-layout-crosshair" fill="none" stroke="#f2eee6" strokeWidth="0.8" d="M54 74v24m-12-12h24" />
        <rect className="studio-layout-hud" fill="#b5a488" x="13" y="155" width="24" height="4" rx="1" />
        <text x="54" y="175" className="studio-layout-label" fill="#f2eee6" fontSize="9" textAnchor="middle">{t('Gameplay')}</text>
        <text x="54" y="185" className="studio-layout-ratio" fill="#9d968a" fontSize="7" textAnchor="middle">{t('9:16 · ohne Balken')}</text>
      </svg>
    );
  }
  return (
    <svg className="studio-layout-schematic" viewBox="0 0 108 192" aria-hidden="true">
      <rect className="studio-layout-background" fill="#161616" stroke="#362c23" x="1" y="1" width="106" height="190" rx="7" />
      {stacked ? (
        <>
          <rect className="studio-layout-camera" fill="#6b4e27" stroke="#c5a059" x="6" y="6" width="96" height="48" rx="3" />
          <circle className="studio-layout-figure" fill="#c5a059" cx="54" cy="24" r="8" />
          <path className="studio-layout-figure" fill="#c5a059" d="M38 46a16 16 0 0 1 32 0" />
          <text x="54" y="65" className="studio-layout-brand" fill="#c5a059" fontSize="7" textAnchor="middle">DDC</text>
        </>
      ) : null}
      <rect className="studio-layout-game" fill="#2a221c" stroke="#7a6c57" x="6" y={stacked ? 74 : 56} width="96" height={stacked ? 88 : 66} rx="3" />
      <path className="studio-layout-scene" fill="#3a2e25" d={stacked ? 'M10 148 31 106 49 125 74 90 98 148Z' : 'M10 108 31 68 49 90 74 65 98 108Z'} />
      <circle className="studio-layout-crosshair" fill="none" stroke="#f2eee6" strokeWidth="0.8" cx="54" cy={stacked ? 118 : 89} r="7" />
      <path className="studio-layout-crosshair" fill="none" stroke="#f2eee6" strokeWidth="0.8" d={stacked ? 'M54 106v24m-12-12h24' : 'M54 77v24m-12-12h24'} />
      <rect className="studio-layout-hud" fill="#b5a488" x="13" y={stacked ? 151 : 111} width="24" height="4" rx="1" />
      <rect className="studio-layout-hud" fill="#b5a488" x="71" y={stacked ? 151 : 111} width="24" height="4" rx="1" />
      {pip && (
        <>
          <rect className="studio-layout-camera" fill="#6b4e27" stroke="#c5a059" x="71" y="9" width="29" height="35" rx="3" />
          <circle className="studio-layout-figure" fill="#c5a059" cx="85.5" cy="21" r="5" />
          <path className="studio-layout-figure" fill="#c5a059" d="M76 38a10 10 0 0 1 19 0" />
        </>
      )}
      <text x="54" y={stacked ? 173 : 145} className="studio-layout-label" fill="#f2eee6" fontSize="9" textAnchor="middle">{t('Gameplay')}</text>
      <text x="54" y="183" className="studio-layout-ratio" fill="#9d968a" fontSize="7" textAnchor="middle">{stacked ? '9:16' : t('9:16 · HUD erhalten')}</text>
    </svg>
  );
}
