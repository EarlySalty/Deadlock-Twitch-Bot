import { test } from 'node:test';
import assert from 'node:assert/strict';

import {
  DEFAULT_PIP_TILE,
  MAX_BAND_HEIGHT,
  MIN_BOX_SIZE,
  TARGET_HEIGHT,
  TARGET_WIDTH,
  applyDrag,
  clampCamPositionToTarget,
  clampToFrame,
  normalizeStoredCamPosition,
  toEvenSize,
  withBandHeight,
} from '../src/utils/socialMediaLayout';
import { DEFAULT_LAYOUT } from '../src/types/socialMedia';

// Der Zielframe ist der Vertrag zwischen Editor und Renderer
// (rust/crates/tb-social-media/src/layout.rs: TARGET_WIDTH/TARGET_HEIGHT).
test('Zielframe ist 1080x1920', () => {
  assert.equal(TARGET_WIDTH, 1080);
  assert.equal(TARGET_HEIGHT, 1920);
});

// Driftschutz: das Default-Layout muss dem Rust-Default entsprechen, sonst
// zeigt die Vorschau etwas anderes als FFmpeg rendert.
test('Default-cam_position ist die PiP-Kachel rechts oben', () => {
  assert.deepEqual(DEFAULT_LAYOUT.cam_position, { x: 712, y: 48, w: 320, h: 320 });
  assert.deepEqual(DEFAULT_LAYOUT.cam_position, DEFAULT_PIP_TILE);
  // 1080 - 320 - 48 = 712: gleicher Rand rechts wie oben.
  assert.equal(
    TARGET_WIDTH - DEFAULT_LAYOUT.cam_position.w - DEFAULT_LAYOUT.cam_position.x,
    DEFAULT_LAYOUT.cam_position.y,
  );
});

test('clampToFrame haelt Boxen im Rahmen und erzwingt eine Mindestgroesse', () => {
  assert.deepEqual(clampToFrame({ x: -50, y: -20, w: 400, h: 300 }, 1920, 1080), {
    x: 0,
    y: 0,
    w: 400,
    h: 300,
  });
  // Ragt rechts raus: wird nach links geschoben, nicht geschrumpft.
  assert.deepEqual(clampToFrame({ x: 1800, y: 900, w: 400, h: 300 }, 1920, 1080), {
    x: 1520,
    y: 780,
    w: 400,
    h: 300,
  });
  // Groesser als der Rahmen: auf Rahmengroesse gestutzt.
  assert.deepEqual(clampToFrame({ x: 10, y: 10, w: 5000, h: 5000 }, 1920, 1080), {
    x: 0,
    y: 0,
    w: 1920,
    h: 1080,
  });
  // Zu klein: Mindestgroesse.
  assert.equal(clampToFrame({ x: 0, y: 0, w: 3, h: 3 }, 1920, 1080).w, MIN_BOX_SIZE);
  // Ganzzahlig, damit der API-Payload keine Nachkommastellen traegt.
  const gerundet = clampToFrame({ x: 10.4, y: 20.6, w: 300.5, h: 200.4 }, 1920, 1080);
  assert.deepEqual(gerundet, { x: 10, y: 21, w: 301, h: 200 });
});

// Spiegelt exakt LayoutBox::clamped_to_target in layout.rs. Laeuft die eine
// Seite weg, zeigt der Editor etwas anderes an, als das Backend speichert.
test('clampCamPositionToTarget spiegelt das Backend-Clamping', () => {
  assert.deepEqual(clampCamPositionToTarget({ x: 126, y: 0, w: 1080, h: 540 }), {
    x: 0,
    y: 0,
    w: 1080,
    h: 540,
  });
  assert.deepEqual(clampCamPositionToTarget({ x: 900, y: 1800, w: 600, h: 400 }), {
    x: 480,
    y: 1520,
    w: 600,
    h: 400,
  });
  // Gueltiges Zielrechteck bleibt unangetastet.
  assert.deepEqual(clampCamPositionToTarget({ x: 712, y: 48, w: 320, h: 320 }), {
    x: 712,
    y: 48,
    w: 320,
    h: 320,
  });
});

test('applyDrag verschiebt und skaliert an der gezogenen Ecke', () => {
  const start = { x: 100, y: 100, w: 400, h: 300 };
  assert.deepEqual(applyDrag(start, 50, -30, 'move'), { x: 150, y: 70, w: 400, h: 300 });
  assert.deepEqual(applyDrag(start, 50, 40, 'resize-br'), { x: 100, y: 100, w: 450, h: 340 });
  // Oben links ziehen verschiebt den Ursprung und aendert die Groesse gegenlaeufig.
  assert.deepEqual(applyDrag(start, 20, 10, 'resize-tl'), { x: 120, y: 110, w: 380, h: 290 });
  assert.deepEqual(applyDrag(start, 20, 10, 'resize-tr'), { x: 100, y: 110, w: 420, h: 290 });
  assert.deepEqual(applyDrag(start, 20, 10, 'resize-bl'), { x: 120, y: 100, w: 380, h: 310 });
  // Unterkante: nur die Hoehe.
  assert.deepEqual(applyDrag(start, 999, 60, 'resize-b'), { x: 100, y: 100, w: 400, h: 360 });
});

// Im Stacked-Modus zieht der Nutzer nur die Streifenhoehe. x/y/w bleiben stehen,
// weil der Renderer sie ignoriert und der Nutzer sie beim Wechsel zurueck nach
// PiP wiederhaben will.
test('withBandHeight aendert nur die Hoehe und bleibt im Zielframe', () => {
  const kachel = { x: 712, y: 48, w: 320, h: 320 };
  assert.deepEqual(withBandHeight(kachel, 600), { x: 712, y: 48, w: 320, h: 600 });
  // Der Deckel haengt am Renderer (Streifen sitzt immer bei y=0, die Restflaeche
  // darunter braucht 2 px), nicht am gespeicherten y.
  assert.equal(MAX_BAND_HEIGHT, TARGET_HEIGHT - 2);
  const voll = withBandHeight(kachel, 5000);
  assert.equal(voll.h, MAX_BAND_HEIGHT);
  // y wird nachgezogen, sonst lehnt die API das Speichern ab (y + h > 1920).
  assert.ok(voll.y + voll.h <= TARGET_HEIGHT, `y+h=${voll.y + voll.h}`);
  assert.equal(voll.w, kachel.w);
  // Untergrenze, damit der Streifen nicht auf null zusammenfaellt.
  assert.equal(withBandHeight(kachel, 1).h, MIN_BOX_SIZE);
  // Gerade Hoehe, sonst bricht libx264 mit yuv420p ab.
  assert.equal(withBandHeight(kachel, 541).h, 540);
});

// yuv420p vertraegt keine ungeraden Chroma-Maße: scale=421:561 laesst ffmpeg
// abbrechen. Gegenstueck: even_size in layout.rs.
test('Zielrechtecke bekommen gerade Kantenlaengen', () => {
  assert.equal(toEvenSize(321), 320);
  assert.equal(toEvenSize(320), 320);
  assert.equal(toEvenSize(1), 2);
  assert.deepEqual(clampCamPositionToTarget({ x: 100, y: 100, w: 321, h: 201 }), {
    x: 100,
    y: 100,
    w: 320,
    h: 200,
  });
});

// Gegenstueck zu normalize_stored_cam_position in layout.rs: im PiP-Modus war
// cam_position frueher nie editierbar und nie wirksam, ein frameweiter Wert ist
// also Altlast und keine Nutzerentscheidung.
test('normalizeStoredCamPosition faengt die PiP-Altlast ab', () => {
  const altlast = { x: 0, y: 0, w: 1080, h: 540 };
  assert.deepEqual(normalizeStoredCamPosition(altlast, 'pip'), DEFAULT_PIP_TILE);
  // Stacked bleibt: dort war die Streifenhoehe echt gesetzt.
  assert.deepEqual(normalizeStoredCamPosition(altlast, 'stacked'), altlast);
  // Echter, schmaler PiP-Wert kommt unveraendert durch.
  const echt = { x: 40, y: 900, w: 300, h: 300 };
  assert.deepEqual(normalizeStoredCamPosition(echt, 'pip'), echt);
});
