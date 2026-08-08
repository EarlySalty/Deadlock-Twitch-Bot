import type { LayoutBox } from '../types/socialMedia';

/**
 * Reine Geometrie für den Layout-Editor. Zwei Koordinatenräume, die nicht
 * verwechselt werden dürfen:
 *
 * - `game_crop` und `cam_crop` sind Ausschnitte AUS dem Twitch-Bild und liegen
 *   im Quellraum (meist 1920x1080).
 * - `cam_position` ist das Zielrechteck IM fertigen Hochformat-Frame
 *   (TARGET_WIDTH x TARGET_HEIGHT).
 *
 * Gegenstück im Backend: rust/crates/tb-social-media/src/layout.rs.
 */

/** Breite des fertigen Hochformat-Frames (9:16). */
export const TARGET_WIDTH = 1080;
/** Höhe des fertigen Hochformat-Frames (9:16). */
export const TARGET_HEIGHT = 1920;

/** Kleinste ziehbare Kantenlänge, damit eine Box nicht unklickbar wird. */
export const MIN_BOX_SIZE = 80;

/**
 * Cam-Kachel rechts oben: der PiP-Standard, der vor dem Umbau fest im Renderer
 * stand (320 Kantenlänge, 48 Rand). Gegenstück: `DEFAULT_PIP_TILE` in layout.rs.
 */
export const DEFAULT_PIP_TILE: LayoutBox = { x: 712, y: 48, w: 320, h: 320 };

/**
 * Größte Höhe des Cam-Streifens im Stacked-Modus. Der Deckel hängt am Renderer:
 * der Streifen sitzt dort immer bei y=0, und die Game-Fläche darunter braucht
 * noch 2 gerade Pixel.
 */
export const MAX_BAND_HEIGHT = TARGET_HEIGHT - 2;

export type DragMode =
  | 'move'
  | 'resize-tl'
  | 'resize-tr'
  | 'resize-bl'
  | 'resize-br'
  | 'resize-b';

/**
 * Hält eine Box vollständig in einem Rahmen: erst die Größe auf den Rahmen
 * stutzen, dann den Ursprung nachziehen. Rundet auf ganze Pixel, weil die API
 * Integer erwartet.
 */
export function clampToFrame(
  box: LayoutBox,
  frameWidth: number,
  frameHeight: number,
  minSize: number = MIN_BOX_SIZE,
): LayoutBox {
  const w = Math.round(Math.min(Math.max(box.w, minSize), frameWidth));
  const h = Math.round(Math.min(Math.max(box.h, minSize), frameHeight));
  const x = Math.round(Math.min(Math.max(box.x, 0), frameWidth - w));
  const y = Math.round(Math.min(Math.max(box.y, 0), frameHeight - h));
  return { x, y, w, h };
}

/**
 * Rundet eine Kantenlänge auf einen geraden Wert ab (Minimum 2). yuv420p
 * verträgt keine ungeraden Chroma-Maße: `scale=421:561` lässt libx264
 * abbrechen. Gegenstück: `even_size` in layout.rs.
 */
export function toEvenSize(value: number): number {
  const rounded = Math.round(value);
  return Math.max(2, rounded - (rounded % 2));
}

/**
 * Spiegelt `LayoutBox::clamped_to_target` aus layout.rs: gespeicherte Layouts
 * aus der Zeit, als `cam_position` im Quellraum gemeint war, werden in den
 * Zielframe geschoben statt abgelehnt, und die Kantenlängen werden gerade.
 * Damit zeigt der Editor denselben Wert, den das Backend beim Rendern benutzt.
 */
export function clampCamPositionToTarget(box: LayoutBox): LayoutBox {
  const inside = clampToFrame(box, TARGET_WIDTH, TARGET_HEIGHT, 1);
  return { ...inside, w: toEvenSize(inside.w), h: toEvenSize(inside.h) };
}

/**
 * Bringt ein gespeichertes `cam_position` in den Zielframe. Spiegelt
 * `normalize_stored_cam_position` aus layout.rs.
 *
 * Sonderfall PiP: vor dem Umbau war `cam_position` im PiP-Modus weder
 * editierbar noch wirksam, gespeichert wurde trotzdem der Stacked-Default
 * `{0,0,1080,540}`. Ein frameweiter Wert ist dort Altlast, keine Entscheidung
 * des Nutzers, und wird auf den PiP-Standard zurückgesetzt. Im Stacked-Modus
 * war die Streifenhöhe echt gesetzt und bleibt.
 */
export function normalizeStoredCamPosition(box: LayoutBox, mode: string): LayoutBox {
  if (mode === 'pip' && box.w >= TARGET_WIDTH) return { ...DEFAULT_PIP_TILE };
  return clampCamPositionToTarget(box);
}

/** Wendet eine Zeigerbewegung auf die Startbox an. Ohne Rahmenbegrenzung. */
export function applyDrag(
  startBox: LayoutBox,
  dx: number,
  dy: number,
  mode: DragMode,
): LayoutBox {
  switch (mode) {
    case 'move':
      return { ...startBox, x: startBox.x + dx, y: startBox.y + dy };
    case 'resize-br':
      return { ...startBox, w: startBox.w + dx, h: startBox.h + dy };
    case 'resize-tr':
      return { ...startBox, y: startBox.y + dy, w: startBox.w + dx, h: startBox.h - dy };
    case 'resize-bl':
      return { ...startBox, x: startBox.x + dx, w: startBox.w - dx, h: startBox.h + dy };
    case 'resize-tl':
      return {
        x: startBox.x + dx,
        y: startBox.y + dy,
        w: startBox.w - dx,
        h: startBox.h - dy,
      };
    case 'resize-b':
      return { ...startBox, h: startBox.h + dy };
  }
}

/**
 * Setzt nur die Höhe des Cam-Streifens (Stacked-Modus). x und w bleiben stehen,
 * weil der Renderer sie im Streifen-Modus ignoriert und der Nutzer sie beim
 * Wechsel zurück nach PiP wiederhaben will. Die Höhe darf bis [`MAX_BAND_HEIGHT`]
 * gehen; `y` wird nur nachgezogen, damit `y + h` im Zielframe bleibt, sonst
 * lehnt die strenge API-Prüfung das Speichern ab.
 */
export function withBandHeight(camPosition: LayoutBox, height: number): LayoutBox {
  const h = toEvenSize(Math.min(Math.max(height, MIN_BOX_SIZE), MAX_BAND_HEIGHT));
  return {
    ...camPosition,
    y: Math.max(0, Math.min(camPosition.y, TARGET_HEIGHT - h)),
    h,
  };
}

/**
 * Deckelt die Breite der PiP-Kachel knapp unter die Framebreite. Eine Kachel,
 * die den Frame voll ausfüllt, liest [`normalizeStoredCamPosition`] beim
 * nächsten Laden als Altlast und ersetzt sie durch den Standard. Der Nutzer
 * würde also speichern und beim Neuladen etwas anderes sehen.
 */
export function cappedTileWidth(box: LayoutBox): LayoutBox {
  return { ...box, w: Math.min(box.w, TARGET_WIDTH - 2) };
}

/** `320x320 @ (712,48)` — kompakte Anzeige einer Box. */
export function formatBox(box: LayoutBox): string {
  return `${Math.round(box.w)}×${Math.round(box.h)} @ (${Math.round(box.x)},${Math.round(box.y)})`;
}
