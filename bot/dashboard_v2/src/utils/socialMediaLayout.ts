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
 * Spiegelt `LayoutBox::clamped_to_target` aus layout.rs: gespeicherte Layouts
 * aus der Zeit, als `cam_position` im Quellraum gemeint war, werden in den
 * Zielframe geschoben statt abgelehnt. Damit zeigt der Editor denselben Wert,
 * den das Backend beim Rendern benutzt.
 */
export function clampCamPositionToTarget(box: LayoutBox): LayoutBox {
  return clampToFrame(box, TARGET_WIDTH, TARGET_HEIGHT, 1);
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
 * Setzt nur die Höhe des Cam-Streifens (Stacked-Modus). x/y/w bleiben stehen,
 * weil der Renderer sie im Streifen-Modus ignoriert und der Nutzer sie beim
 * Wechsel zurück nach PiP wiederhaben will. Die Höhe bleibt so, dass `y + h`
 * im Zielframe bleibt, sonst lehnt die API das Speichern ab.
 */
export function withBandHeight(camPosition: LayoutBox, height: number): LayoutBox {
  const max = Math.max(MIN_BOX_SIZE, TARGET_HEIGHT - camPosition.y);
  return {
    ...camPosition,
    h: Math.round(Math.min(Math.max(height, MIN_BOX_SIZE), max)),
  };
}

/** `320x320 @ (712,48)` — kompakte Anzeige einer Box. */
export function formatBox(box: LayoutBox): string {
  return `${Math.round(box.w)}×${Math.round(box.h)} @ (${Math.round(box.x)},${Math.round(box.y)})`;
}
