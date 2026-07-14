/*
 * Die Ausstattung der Empfangshalle — als Inline-SVG.
 *
 * Warum SVG statt Bilddateien:
 *   - Die CSP erlaubt nur `img-src 'self' data:` — externe Bilder sind ohnehin tot.
 *   - Es ist eine Linienzeichnung. Als PNG braeuchte sie mehrere Aufloesungen und
 *     bliebe auf jedem Retina-Display weicher als der Rest der Seite.
 *   - Die Farben kommen aus `currentColor` bzw. den Messing-Tokens: aendert sich
 *     die Marke, aendert sich das Mobiliar mit. Ein PNG muesste man neu malen.
 *
 * Alles hier ist Dekoration und traegt keine Information — deshalb durchgaengig
 * aria-hidden. Was der Nutzer wissen muss, steht im Text.
 */

/**
 * Die Rueckwand des Empfangs: das Schluesselfach-Regal.
 *
 * Das ist das eigentliche Erkennungszeichen. Ein Tresen allein ist ein Tresen;
 * erst die Wand mit den Zimmerschluesseln dahinter macht daraus unverwechselbar
 * eine Hotelrezeption. Bewusst sehr zurueckgenommen (Deckkraft im CSS): Kulisse,
 * die man wahrnimmt, aber nicht liest — sonst kaempft sie mit der Antwort um
 * Aufmerksamkeit, und die Antwort muss gewinnen.
 */
export function KeyRackBackdrop({ className = "" }: { className?: string }) {
  /* Dicht und klein. Grosse Faecher lesen sich wie Fenster, nicht wie ein
     Schluesselregal — die Menge macht das Bild, nicht die Groesse. */
  const columns = 14;
  const rows = 4;
  const boxWidth = 30;
  const boxHeight = 27;
  const gapX = 7;
  const gapY = 7;
  const originX = 20;
  const originY = 26;

  /* Nicht in jedem Fach haengt ein Schluessel — belegte Zimmer sind unterwegs.
     Ein vollstaendig gefuelltes Regal wirkt wie ein Muster, ein teilweise
     gefuelltes wie ein Ort, an dem etwas passiert. Fester Rhythmus statt
     Zufall: die Kulisse soll bei jedem Laden gleich aussehen. */
  const hasKey = (index: number) => index % 3 !== 1;

  const boxes = [];
  for (let row = 0; row < rows; row += 1) {
    for (let column = 0; column < columns; column += 1) {
      const index = row * columns + column;
      const x = originX + column * (boxWidth + gapX);
      const y = originY + row * (boxHeight + gapY);
      boxes.push(
        <g key={index}>
          {/* Fach */}
          <rect
            x={x}
            y={y}
            width={boxWidth}
            height={boxHeight}
            rx="2"
            fill="rgba(0,0,0,0.28)"
            stroke="currentColor"
            strokeWidth="1"
          />
          {/* Zimmernummer als Praegung, nur angedeutet */}
          <line
            x1={x + 5}
            y1={y + 6}
            x2={x + 13}
            y2={y + 6}
            stroke="currentColor"
            strokeWidth="1.3"
            strokeLinecap="round"
            opacity="0.5"
          />
          {hasKey(index) ? (
            /* Schluessel am Haken: Ring, Schaft, zwei Baerte.
               Koordinaten haengen an boxHeight — sonst ragt der Schluessel aus
               dem Fach, sobald jemand das Regal enger stellt. */
            <g stroke="currentColor" strokeWidth="1.2" fill="none" strokeLinecap="round">
              <circle cx={x + boxWidth / 2} cy={y + boxHeight * 0.45} r="3" />
              <line
                x1={x + boxWidth / 2}
                y1={y + boxHeight * 0.45 + 3}
                x2={x + boxWidth / 2}
                y2={y + boxHeight - 4}
              />
              <line
                x1={x + boxWidth / 2}
                y1={y + boxHeight - 7}
                x2={x + boxWidth / 2 + 3.4}
                y2={y + boxHeight - 7}
              />
              <line
                x1={x + boxWidth / 2}
                y1={y + boxHeight - 4}
                x2={x + boxWidth / 2 + 2.6}
                y2={y + boxHeight - 4}
              />
            </g>
          ) : null}
        </g>,
      );
    }
  }

  const rackWidth = originX * 2 + columns * boxWidth + (columns - 1) * gapX;
  const rackHeight = originY + rows * boxHeight + (rows - 1) * gapY + 22;

  return (
    <svg
      aria-hidden="true"
      focusable="false"
      viewBox={`0 0 ${rackWidth} ${rackHeight}`}
      preserveAspectRatio="xMidYMid slice"
      className={className}
    >
      {/* Traegerbalken oben — das Regal haengt an der Wand, es schwebt nicht */}
      <line
        x1="8"
        y1={originY - 12}
        x2={rackWidth - 8}
        y2={originY - 12}
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
      />
      {boxes}
    </svg>
  );
}

/**
 * Der Concierge.
 *
 * Bewusst ohne Augen: ein Portier ist diskret, er mustert niemanden. Erkennbar
 * ist er an der Uniform — Tellermuetze mit Schirm, hoher Kragen, Doppelreihe
 * Messingknoepfe, Epauletten. Genau die Merkmale, an denen man die Rolle auch
 * dann erkennt, wenn das Gesicht im Schatten bleibt.
 */
export function DoormanPortrait({ className = "" }: { className?: string }) {
  /*
   * Zweiter Anlauf. Der erste zerfiel in zwei Teile: die Zeichenflaeche war
   * 120x132, gerendert wurde sie in ein QUADRAT — und der Hals war fast so
   * dunkel wie der Tresen dahinter. Ergebnis: ein Kopf, der ueber einem Faecher
   * schwebte. Jetzt quadratische viewBox (die Figur fuellt sie aus) und ein Hals
   * in Kopffarbe mit Kontur, damit die Silhouette geschlossen bleibt.
   */
  return (
    <svg aria-hidden="true" focusable="false" viewBox="0 0 120 120" className={className} fill="none">
      <defs>
        <linearGradient id="doorman-brass" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="var(--brass-bright, #efd49d)" />
          <stop offset="100%" stopColor="var(--brass-deep, #8a7038)" />
        </linearGradient>
        <linearGradient id="doorman-coat" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="#46351f" />
          <stop offset="100%" stopColor="#241b10" />
        </linearGradient>
      </defs>

      {/* Hals — in Kopffarbe, mit Kontur. Vorher unsichtbar gegen den Tresen. */}
      <path
        d="M50 62 L50 88 L70 88 L70 62 Z"
        fill="#40321f"
        stroke="var(--brass, #c8a86b)"
        strokeWidth="1.6"
      />

      {/* Schultern */}
      <path
        d="M20 120 C20 100 34 84 60 84 C86 84 100 100 100 120 Z"
        fill="url(#doorman-coat)"
        stroke="var(--brass, #c8a86b)"
        strokeWidth="2"
      />

      {/* Revers */}
      <path
        d="M60 88 L49 120 M60 88 L71 120"
        stroke="var(--brass, #c8a86b)"
        strokeWidth="1.8"
        opacity="0.8"
      />

      {/* Doppelreihe Messingknoepfe */}
      {[102, 113].map((cy) => (
        <g key={cy}>
          <circle cx="52" cy={cy} r="2.8" fill="url(#doorman-brass)" />
          <circle cx="68" cy={cy} r="2.8" fill="url(#doorman-brass)" />
        </g>
      ))}

      {/* Epauletten */}
      <path
        d="M24 101 L36 95 M96 101 L84 95"
        stroke="url(#doorman-brass)"
        strokeWidth="4"
        strokeLinecap="round"
      />

      {/* Hoher Kragen, schliesst Hals und Jacke zusammen */}
      <path
        d="M45 86 C49 94 54 98 60 98 C66 98 71 94 75 86"
        stroke="var(--brass, #c8a86b)"
        strokeWidth="2.2"
        fill="none"
      />

      {/* Kopf */}
      <ellipse cx="60" cy="50" rx="18" ry="20" fill="#40321f" stroke="var(--brass, #c8a86b)" strokeWidth="2" />

      {/* Schnurrbart — das einzige Gesichtsmerkmal, das er zeigt */}
      <path
        d="M51 59 C55 56 58 56 60 57 C62 56 65 56 69 59"
        stroke="var(--brass, #c8a86b)"
        strokeWidth="2.6"
        strokeLinecap="round"
        fill="none"
      />

      {/* Tellermuetze: Schirm, Band, Deckel */}
      <path
        d="M35 35 C39 33 47 32 60 32 C73 32 81 33 85 35 L85 38.5 C77 36.5 68 35.5 60 35.5 C52 35.5 43 36.5 35 38.5 Z"
        fill="url(#doorman-brass)"
      />
      <rect x="38" y="22" width="44" height="11" rx="2" fill="#241b10" stroke="var(--brass, #c8a86b)" strokeWidth="1.8" />
      <path
        d="M41 22 C41 11 49 6 60 6 C71 6 79 11 79 22 Z"
        fill="#40321f"
        stroke="var(--brass, #c8a86b)"
        strokeWidth="2"
      />
      {/* Kokarde am Muetzenband */}
      <circle cx="60" cy="27.5" r="3.6" fill="url(#doorman-brass)" />
    </svg>
  );
}

/**
 * Der Concierge im Kleinformat — steht neben jeder Auskunft, damit erkennbar
 * bleibt, wer spricht. Nur Muetze und Kragen, alles andere ginge bei dieser
 * Groesse ohnehin zu Brei.
 */
export function DoormanBadge({ className = "" }: { className?: string }) {
  /*
   * Der erste Entwurf hatte hier Schultern, Revers und Kragen — bei 36 px wurde
   * daraus ein Knaeuel aus Kringeln, in dem nichts mehr zu erkennen war. Was auf
   * dieser Groesse traegt, ist nur die Silhouette: Muetze und Schnurrbart. Alles
   * andere ist raus, dafuer sitzt der Kopf gross im Medaillon.
   */
  return (
    <svg aria-hidden="true" focusable="false" viewBox="0 0 40 40" className={className} fill="none">
      {/* Messing-Medaillon */}
      <circle cx="20" cy="20" r="18.4" fill="#2c2113" stroke="var(--brass, #c8a86b)" strokeWidth="1.6" />

      {/* Kopf — nimmt fast das ganze Medaillon ein, sonst verschwindet er */}
      <ellipse cx="20" cy="24" rx="9" ry="9.6" fill="#40321f" stroke="var(--brass, #c8a86b)" strokeWidth="1.5" />

      {/* Schnurrbart */}
      <path
        d="M15.6 27.4 C17.4 25.9 19 25.9 20 26.4 C21 25.9 22.6 25.9 24.4 27.4"
        stroke="var(--brass, #c8a86b)"
        strokeWidth="1.7"
        strokeLinecap="round"
      />

      {/* Tellermuetze: Schirm, Band, Deckel */}
      <path
        d="M9.5 17.2 C11.6 16.3 15 15.8 20 15.8 C25 15.8 28.4 16.3 30.5 17.2 L30.5 18.8 C26.6 17.7 23.4 17.2 20 17.2 C16.6 17.2 13.4 17.7 9.5 18.8 Z"
        fill="var(--brass, #c8a86b)"
      />
      <rect x="11.6" y="11.6" width="16.8" height="4.6" rx="1" fill="#241b10" stroke="var(--brass, #c8a86b)" strokeWidth="1.3" />
      <path
        d="M12.8 11.6 C12.8 7.2 15.8 5.2 20 5.2 C24.2 5.2 27.2 7.2 27.2 11.6 Z"
        fill="#40321f"
        stroke="var(--brass, #c8a86b)"
        strokeWidth="1.4"
      />
      <circle cx="20" cy="13.9" r="1.5" fill="var(--brass-bright, #efd49d)" />
    </svg>
  );
}
