export interface Frage {
  id: string;
  frage: string;
  antwort: string;
}

export const PUBLIC_API_BASE =
  "https://deutsche-deadlock-community.de/twitch/api/v2/public";

export const HAEUFIGE_FRAGEN: Frage[] = [
  {
    id: "kosten",
    frage: "Was kostet die Teilnahme am Partner-Netzwerk?",
    antwort:
      "Nichts. Netzwerk Free kostet 0 €, ohne Abo und ohne Laufzeit. Auto-Raid in beide Richtungen, Chat-Schutz, alle Befehle, Overlay, Sendeplan und Go-Live im Community-Discord sind dauerhaft enthalten. Kostenpflichtig ist nur die freiwillige Zusatzstufe Netzwerk Plus für 4,99 € im Monat oder 49,90 € im Jahr. Alle Beträge sind Endpreise.",
  },
  {
    id: "testphase",
    frage: "Gibt es eine kostenlose Testphase oder eine Demo?",
    antwort:
      "Ja, beides. Das Netzwerk selbst ist dauerhaft kostenlos und braucht deshalb keine Testphase. Netzwerk Plus kannst du im Monatsabo 30 Tage kostenlos testen, einmal pro Kanal. Kündigst du vor Ablauf, wird nichts berechnet. Ohne Anmeldung zeigt dir das Demo-Dashboard alle Auswertungen mit Beispieldaten.",
  },
  {
    id: "kuendigung",
    frage: "Wie kündige ich, und gibt es eine Erstattung?",
    antwort:
      "Die kostenlose Teilnahme beendest du jederzeit ohne Frist: Trenne den Bot in deinen Twitch-Einstellungen unter Verbindungen oder schalte ihn in der Selbstverwaltung ab. Ein bezahltes Abo kündigst du in der Abo-Verwaltung im Dashboard oder per E-Mail. Es läuft bis zum Ende des bezahlten Zeitraums und verlängert sich danach nicht mehr, dein Kanal fällt dann auf Netzwerk Free zurück. Eine anteilige Erstattung für den laufenden Zeitraum gibt es nicht. Die genauen Regeln stehen in § 9 der AGB.",
  },
  {
    id: "daten",
    frage: "Was passiert mit meinen Daten, wenn ich gehe?",
    antwort:
      "Nach dem Trennen nutzt der Bot die Twitch-Verbindung für keine weitere Aktion. Die Löschung deiner gespeicherten Daten kannst du zusätzlich verlangen. Ausgenommen sind nur Rechnungsdaten, die wir gesetzlich aufbewahren müssen. E-Mail-Adresse, Passwort oder Zahlungsdaten für die kostenlose Teilnahme haben wir ohnehin nicht.",
  },
  {
    id: "schnittstellen",
    frage: "Welche Integrationen und Schnittstellen gibt es?",
    antwort:
      "Der Bot arbeitet mit Twitch (Raids, Live-Erkennung, Moderation, Clips) und Discord (Go-Live-Ankündigungen, Community). Für Entwickler gibt es öffentliche, rein lesende JSON-Endpunkte ohne Schlüssel, zum Beispiel für Live-Partner, Netzwerk-Kennzahlen und zuletzt gebannte Spam-Konten. Eine schreibende Entwickler-API bieten wir nicht an.",
  },
  {
    id: "zielgruppe",
    frage: "Für wen ist das Partner-Netzwerk gedacht?",
    antwort:
      "Für deutschsprachige Twitch-Streamer, die Deadlock spielen. Auto-Raids laufen nur, solange du Deadlock streamst.",
  },
];
